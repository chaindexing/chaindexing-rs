use alloy::consensus::Transaction as _;
use alloy::network::primitives::BlockTransactions;
use alloy::network::TransactionResponse;
use alloy::primitives::B256;
use alloy::rpc::types::Block;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chain_blocks::h256_to_string;
use crate::ChainId;

/// Opt-in lower-level data captured alongside event indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDataConfig {
    /// Store full JSON-RPC transaction payloads in `chaindexing_transactions`.
    pub raw_transactions: bool,
    /// Store call trace payloads in `chaindexing_call_traces`.
    pub call_traces: bool,
}

impl IndexedDataConfig {
    /// Keeps the default event-only indexing behavior.
    pub fn none() -> Self {
        Self {
            raw_transactions: false,
            call_traces: false,
        }
    }

    /// Enables every optional indexed-data surface.
    pub fn all() -> Self {
        Self {
            raw_transactions: true,
            call_traces: true,
        }
    }

    /// Enables full transaction payload indexing.
    pub fn with_raw_transactions(mut self) -> Self {
        self.raw_transactions = true;
        self
    }

    /// Enables call trace indexing.
    pub fn with_call_traces(mut self) -> Self {
        self.call_traces = true;
        self
    }

    /// Returns whether ingestion must fetch full block transaction objects.
    pub fn requires_full_blocks(&self) -> bool {
        self.raw_transactions
    }

    /// Returns whether any optional indexed-data surface is enabled.
    pub fn enabled(&self) -> bool {
        self.raw_transactions || self.call_traces
    }
}

impl Default for IndexedDataConfig {
    fn default() -> Self {
        Self::none()
    }
}

/// A transaction row prepared for `chaindexing_transactions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedTransaction {
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub block_timestamp: i64,
    pub transaction_hash: String,
    pub transaction_index: i32,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: String,
    pub input: String,
    pub raw: Value,
}

/// A call trace row prepared for `chaindexing_call_traces`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedCallTrace {
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub transaction_hash: String,
    pub trace_index: i32,
    pub trace_address: String,
    pub call_type: Option<String>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub value: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub raw: Value,
}

pub(crate) fn transactions_from_provider_blocks<'a>(
    chain_id: &ChainId,
    blocks: impl IntoIterator<Item = &'a Block>,
) -> Vec<IndexedTransaction> {
    let mut transactions = Vec::new();

    for block in blocks {
        if block.header.hash == B256::ZERO {
            continue;
        }

        let block_number = block.header.inner.number;
        let block_hash = h256_to_string(&block.header.hash);
        let block_timestamp = block.header.inner.timestamp as i64;

        let BlockTransactions::Full(block_transactions) = &block.transactions else {
            continue;
        };

        for (index, transaction) in block_transactions.iter().enumerate() {
            let block_number = transaction.block_number().unwrap_or(block_number);
            let block_hash = transaction
                .block_hash()
                .map(|hash| h256_to_string(&hash))
                .unwrap_or_else(|| block_hash.clone());
            let transaction_index = transaction.transaction_index().unwrap_or(index as u64);

            transactions.push(IndexedTransaction {
                chain_id: *chain_id as i64,
                block_number: block_number as i64,
                block_hash,
                block_timestamp,
                transaction_hash: h256_to_string(&transaction.tx_hash()),
                transaction_index: transaction_index as i32,
                from_address: transaction.from().to_string().to_lowercase(),
                to_address: transaction.to().map(|address| address.to_string().to_lowercase()),
                value: transaction.value().to_string(),
                input: transaction.input().to_string(),
                raw: serde_json::to_value(transaction).unwrap_or_else(|error| {
                    serde_json::json!({
                        "serialization_error": error.to_string(),
                        "transaction_hash": transaction.tx_hash().to_string()
                    })
                }),
            });
        }
    }

    transactions.sort_by_key(|transaction| {
        (
            transaction.block_number,
            transaction.transaction_index,
            transaction.transaction_hash.clone(),
        )
    });
    transactions
}

pub(crate) fn call_traces_from_provider_traces(
    chain_id: &ChainId,
    block: &Block,
    traces: Vec<Value>,
) -> Vec<IndexedCallTrace> {
    let block_number = block.header.inner.number as i64;
    let block_hash = h256_to_string(&block.header.hash);

    let mut indexed_traces = vec![];

    for (trace_index, trace) in traces.into_iter().enumerate() {
        let transaction_hash = string_field(&trace, &["txHash", "transactionHash"])
            .unwrap_or_default()
            .to_lowercase();
        let trace_address = trace_address(&trace).unwrap_or_else(|| trace_index.to_string());

        flatten_call_trace(
            &mut indexed_traces,
            *chain_id as i64,
            block_number,
            &block_hash,
            &transaction_hash,
            &trace_address,
            trace,
        );
    }

    indexed_traces
}

fn flatten_call_trace(
    indexed_traces: &mut Vec<IndexedCallTrace>,
    chain_id: i64,
    block_number: i64,
    block_hash: &str,
    transaction_hash: &str,
    trace_address: &str,
    trace: Value,
) {
    let trace_index = indexed_traces.len() as i32;
    indexed_traces.push(IndexedCallTrace {
        chain_id,
        block_number,
        block_hash: block_hash.to_string(),
        transaction_hash: transaction_hash.to_string(),
        trace_index,
        trace_address: trace_address.to_string(),
        call_type: string_field(&trace, &["type", "callType"]),
        from_address: string_field(&trace, &["from"]).map(|value| value.to_lowercase()),
        to_address: string_field(&trace, &["to"]).map(|value| value.to_lowercase()),
        value: string_field(&trace, &["value"]),
        input: string_field(&trace, &["input"]),
        output: string_field(&trace, &["output"]),
        error: string_field(&trace, &["error"]),
        raw: trace.clone(),
    });

    let Some(calls) = trace_payload(&trace).get("calls").and_then(Value::as_array) else {
        return;
    };

    for (call_index, call) in calls.iter().cloned().enumerate() {
        flatten_call_trace(
            indexed_traces,
            chain_id,
            block_number,
            block_hash,
            transaction_hash,
            &format!("{trace_address}.{call_index}"),
            call,
        );
    }
}

fn trace_payload(trace: &Value) -> &Value {
    trace.get("result").unwrap_or(trace)
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = value.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }

        if let Some(value) =
            value.get("result").and_then(|result| result.get(key)).and_then(Value::as_str)
        {
            return Some(value.to_string());
        }
    }

    None
}

fn trace_address(trace: &Value) -> Option<String> {
    if let Some(value) = trace.get("traceAddress").and_then(Value::as_str) {
        return Some(value.to_string());
    }

    trace.get("traceAddress").and_then(Value::as_array).map(|address| {
        address
            .iter()
            .filter_map(Value::as_u64)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(".")
    })
}

pub(crate) fn upsert_transactions_query(transactions: &[IndexedTransaction]) -> Option<String> {
    if transactions.is_empty() {
        return None;
    }

    Some(format!(
        "INSERT INTO chaindexing_transactions
            (chain_id, block_number, block_hash, block_timestamp, transaction_hash,
             transaction_index, from_address, to_address, value, input, raw, status)
         VALUES {}
         ON CONFLICT (chain_id, block_hash, transaction_hash)
         DO UPDATE SET
            block_number = EXCLUDED.block_number,
            block_timestamp = EXCLUDED.block_timestamp,
            transaction_index = EXCLUDED.transaction_index,
            from_address = EXCLUDED.from_address,
            to_address = EXCLUDED.to_address,
            value = EXCLUDED.value,
            input = EXCLUDED.input,
            raw = EXCLUDED.raw,
            status = EXCLUDED.status,
            reorg_id = NULL",
        transaction_values(transactions)
    ))
}

pub(crate) fn mark_transactions_reorged_from_query(chain_id: ChainId, block_number: i64) -> String {
    format!(
        "UPDATE chaindexing_transactions
         SET status = 'reorged'
         WHERE chain_id = {}
           AND block_number >= {}
           AND status = 'canonical'",
        chain_id as i64, block_number
    )
}

pub(crate) fn upsert_call_traces_query(traces: &[IndexedCallTrace]) -> Option<String> {
    if traces.is_empty() {
        return None;
    }

    Some(format!(
        "INSERT INTO chaindexing_call_traces
            (chain_id, block_number, block_hash, transaction_hash, trace_index,
             trace_address, call_type, from_address, to_address, value, input, output,
             error, raw, status)
         VALUES {}
         ON CONFLICT (chain_id, block_hash, transaction_hash, trace_address, trace_index)
         DO UPDATE SET
            block_number = EXCLUDED.block_number,
            call_type = EXCLUDED.call_type,
            from_address = EXCLUDED.from_address,
            to_address = EXCLUDED.to_address,
            value = EXCLUDED.value,
            input = EXCLUDED.input,
            output = EXCLUDED.output,
            error = EXCLUDED.error,
            raw = EXCLUDED.raw,
            status = EXCLUDED.status,
            reorg_id = NULL",
        call_trace_values(traces)
    ))
}

pub(crate) fn mark_call_traces_reorged_from_query(chain_id: ChainId, block_number: i64) -> String {
    format!(
        "UPDATE chaindexing_call_traces
         SET status = 'reorged'
         WHERE chain_id = {}
           AND block_number >= {}
           AND status = 'canonical'",
        chain_id as i64, block_number
    )
}

fn transaction_values(transactions: &[IndexedTransaction]) -> String {
    transactions
        .iter()
        .map(|transaction| {
            format!(
                "({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}::jsonb, 'canonical')",
                transaction.chain_id,
                transaction.block_number,
                sql_string_literal(&transaction.block_hash),
                transaction.block_timestamp,
                sql_string_literal(&transaction.transaction_hash),
                transaction.transaction_index,
                sql_string_literal(&transaction.from_address),
                optional_sql_string_literal(transaction.to_address.as_deref()),
                sql_string_literal(&transaction.value),
                sql_string_literal(&transaction.input),
                sql_json_literal(&transaction.raw),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn call_trace_values(traces: &[IndexedCallTrace]) -> String {
    traces
        .iter()
        .map(|trace| {
            format!(
                "({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}::jsonb, 'canonical')",
                trace.chain_id,
                trace.block_number,
                sql_string_literal(&trace.block_hash),
                sql_string_literal(&trace.transaction_hash),
                trace.trace_index,
                sql_string_literal(&trace.trace_address),
                optional_sql_string_literal(trace.call_type.as_deref()),
                optional_sql_string_literal(trace.from_address.as_deref()),
                optional_sql_string_literal(trace.to_address.as_deref()),
                optional_sql_string_literal(trace.value.as_deref()),
                optional_sql_string_literal(trace.input.as_deref()),
                optional_sql_string_literal(trace.output.as_deref()),
                optional_sql_string_literal(trace.error.as_deref()),
                sql_json_literal(&trace.raw),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn optional_sql_string_literal(value: Option<&str>) -> String {
    value.map(sql_string_literal).unwrap_or_else(|| "NULL".to_string())
}

fn sql_json_literal(value: &Value) -> String {
    sql_string_literal(&serde_json::to_string(value).unwrap())
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::consensus::Header as ConsensusHeader;
    use alloy::rpc::types::Header as RpcHeader;

    fn block(number: u64, hash: B256) -> Block {
        Block {
            header: RpcHeader {
                hash,
                inner: ConsensusHeader {
                    number,
                    timestamp: number + 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn h256(value: u64) -> B256 {
        let mut bytes = [0_u8; 32];
        bytes[24..].copy_from_slice(&value.to_be_bytes());
        B256::from(bytes)
    }

    #[test]
    fn indexed_data_config_defaults_to_event_only_indexing() {
        let config = IndexedDataConfig::default();

        assert!(!config.enabled());
        assert!(!config.raw_transactions);
        assert!(!config.call_traces);
        assert!(!config.requires_full_blocks());
    }

    #[test]
    fn transaction_upsert_query_escapes_strings_and_json() {
        let query = upsert_transactions_query(&[IndexedTransaction {
            chain_id: 1,
            block_number: 10,
            block_hash: "0xblock".to_string(),
            block_timestamp: 11,
            transaction_hash: "0xtx".to_string(),
            transaction_index: 0,
            from_address: "0xfrom".to_string(),
            to_address: Some("0xto's".to_string()),
            value: "1".to_string(),
            input: "0xdead'beef".to_string(),
            raw: serde_json::json!({ "note": "owner's tx" }),
        }])
        .unwrap();

        assert!(query.contains("INSERT INTO chaindexing_transactions"));
        assert!(query.contains("'0xto''s'"));
        assert!(query.contains("'0xdead''beef'"));
        assert!(query.contains("{\"note\":\"owner''s tx\"}'::jsonb"));
        assert!(query.contains("ON CONFLICT (chain_id, block_hash, transaction_hash)"));
    }

    #[test]
    fn empty_transaction_upsert_has_no_query() {
        assert_eq!(upsert_transactions_query(&[]), None);
    }

    #[test]
    fn call_trace_upsert_query_uses_stable_identity_and_reorg_status() {
        let query = upsert_call_traces_query(&[IndexedCallTrace {
            chain_id: 1,
            block_number: 10,
            block_hash: "0xblock".to_string(),
            transaction_hash: "0xtx".to_string(),
            trace_index: 2,
            trace_address: "0.1".to_string(),
            call_type: Some("CALL".to_string()),
            from_address: Some("0xfrom".to_string()),
            to_address: None,
            value: None,
            input: Some("0x".to_string()),
            output: None,
            error: Some("execution reverted".to_string()),
            raw: serde_json::json!({ "traceAddress": [0, 1] }),
        }])
        .unwrap();

        assert!(query.contains("INSERT INTO chaindexing_call_traces"));
        assert!(query.contains(
            "ON CONFLICT (chain_id, block_hash, transaction_hash, trace_address, trace_index)"
        ));
        assert!(query.contains("'execution reverted'"));
        assert!(query.contains("status = EXCLUDED.status"));
    }

    #[test]
    fn call_trace_conversion_extracts_common_geth_shapes() {
        let block = block(10, h256(10));
        let traces = call_traces_from_provider_traces(
            &ChainId::Mainnet,
            &block,
            vec![serde_json::json!({
                "txHash": "0xABC",
                "traceAddress": [0, 2],
                "result": {
                    "type": "CALL",
                    "from": "0xFROM",
                    "to": "0xTO",
                    "output": "0x01"
                }
            })],
        );

        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].transaction_hash, "0xabc");
        assert_eq!(traces[0].trace_address, "0.2");
        assert_eq!(traces[0].call_type.as_deref(), Some("CALL"));
        assert_eq!(traces[0].from_address.as_deref(), Some("0xfrom"));
    }

    #[test]
    fn call_trace_conversion_flattens_nested_geth_call_tracer_calls() {
        let block = block(10, h256(10));
        let traces = call_traces_from_provider_traces(
            &ChainId::Mainnet,
            &block,
            vec![serde_json::json!({
                "txHash": "0xABC",
                "result": {
                    "type": "CALL",
                    "from": "0xROOT",
                    "to": "0xA",
                    "calls": [
                        {
                            "type": "STATICCALL",
                            "from": "0xA",
                            "to": "0xB",
                            "calls": [
                                {
                                    "type": "DELEGATECALL",
                                    "from": "0xB",
                                    "to": "0xC"
                                }
                            ]
                        },
                        {
                            "type": "CALL",
                            "from": "0xA",
                            "to": "0xD"
                        }
                    ]
                }
            })],
        );

        assert_eq!(traces.len(), 4);
        assert_eq!(traces[0].trace_address, "0");
        assert_eq!(traces[1].trace_address, "0.0");
        assert_eq!(traces[2].trace_address, "0.0.0");
        assert_eq!(traces[3].trace_address, "0.1");
        assert_eq!(traces[2].call_type.as_deref(), Some("DELEGATECALL"));
        assert!(traces.iter().all(|trace| trace.transaction_hash == "0xabc"));
    }

    #[test]
    fn reorg_queries_scope_to_chain_and_block() {
        assert!(mark_transactions_reorged_from_query(ChainId::Mainnet, 42).contains("chain_id = 1"));
        assert!(mark_call_traces_reorged_from_query(ChainId::Mainnet, 42)
            .contains("block_number >= 42"));
    }
}
