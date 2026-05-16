pub const DEFAULT_HANDLER_ID: &str = "default";

#[derive(Clone, Copy)]
pub enum CheckpointKind {
    Ingestion,
    Reducer,
    SideEffect,
}

impl CheckpointKind {
    fn as_str(self) -> &'static str {
        match self {
            CheckpointKind::Ingestion => "ingestion",
            CheckpointKind::Reducer => "reducer",
            CheckpointKind::SideEffect => "side_effect",
        }
    }
}

pub fn upsert_query(
    chain_id: u64,
    contract_address: &str,
    handler_kind: CheckpointKind,
    next_block_number: u64,
) -> String {
    format!(
        "INSERT INTO chaindexing_checkpoints
        (chain_id, contract_address, handler_kind, handler_id, next_block_number)
        VALUES ({chain_id}, {contract_address}, {handler_kind}, {handler_id}, {next_block_number})
        ON CONFLICT (chain_id, contract_address, handler_kind, handler_id)
        DO UPDATE SET
            next_block_number = EXCLUDED.next_block_number,
            updated_at = NOW()",
        contract_address = to_sql_string_literal(contract_address),
        handler_kind = to_sql_string_literal(handler_kind.as_str()),
        handler_id = to_sql_string_literal(DEFAULT_HANDLER_ID),
    )
}

pub fn upsert_all_for_chain_query(
    chain_id: u64,
    handler_kind: CheckpointKind,
    next_block_number: u64,
) -> String {
    format!(
        "INSERT INTO chaindexing_checkpoints
        (chain_id, contract_address, handler_kind, handler_id, next_block_number)
        SELECT chain_id, address, {handler_kind}, {handler_id}, {next_block_number}
        FROM chaindexing_contract_addresses
        WHERE chain_id = {chain_id}
        ON CONFLICT (chain_id, contract_address, handler_kind, handler_id)
        DO UPDATE SET
            next_block_number = EXCLUDED.next_block_number,
            updated_at = NOW()",
        handler_kind = to_sql_string_literal(handler_kind.as_str()),
        handler_id = to_sql_string_literal(DEFAULT_HANDLER_ID),
    )
}

pub fn contract_addresses_select_query(chain_id: i64, from: i64, to: i64) -> String {
    format!(
        "SELECT
            ca.id,
            ca.chain_id,
            COALESCE(ingestion_checkpoint.next_block_number, ca.next_block_number_to_ingest_from)
                AS next_block_number_to_ingest_from,
            COALESCE(reducer_checkpoint.next_block_number, ca.next_block_number_to_handle_from)
                AS next_block_number_to_handle_from,
            COALESCE(side_effect_checkpoint.next_block_number, ca.next_block_number_for_side_effects)
                AS next_block_number_for_side_effects,
            ca.start_block_number,
            ca.address,
            ca.contract_name
        FROM chaindexing_contract_addresses ca
        LEFT JOIN chaindexing_checkpoints ingestion_checkpoint
            ON ingestion_checkpoint.chain_id = ca.chain_id
            AND ingestion_checkpoint.contract_address = ca.address
            AND ingestion_checkpoint.handler_kind = {ingestion_kind}
            AND ingestion_checkpoint.handler_id = {handler_id}
        LEFT JOIN chaindexing_checkpoints reducer_checkpoint
            ON reducer_checkpoint.chain_id = ca.chain_id
            AND reducer_checkpoint.contract_address = ca.address
            AND reducer_checkpoint.handler_kind = {reducer_kind}
            AND reducer_checkpoint.handler_id = {handler_id}
        LEFT JOIN chaindexing_checkpoints side_effect_checkpoint
            ON side_effect_checkpoint.chain_id = ca.chain_id
            AND side_effect_checkpoint.contract_address = ca.address
            AND side_effect_checkpoint.handler_kind = {side_effect_kind}
            AND side_effect_checkpoint.handler_id = {handler_id}
        WHERE ca.chain_id = {chain_id} AND ca.id BETWEEN {from} AND {to}",
        ingestion_kind = to_sql_string_literal(CheckpointKind::Ingestion.as_str()),
        reducer_kind = to_sql_string_literal(CheckpointKind::Reducer.as_str()),
        side_effect_kind = to_sql_string_literal(CheckpointKind::SideEffect.as_str()),
        handler_id = to_sql_string_literal(DEFAULT_HANDLER_ID),
    )
}

fn to_sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_checkpoint_literals() {
        let query = upsert_query(1, "0xabc'def", CheckpointKind::Ingestion, 10);

        assert!(query.contains("'0xabc''def'"));
    }
}
