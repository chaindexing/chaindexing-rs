use std::collections::HashMap;

use diesel::QueryableByName;
use ethers::types::{Block, TxHash, H256, U64};

use crate::ChainId;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ChainBlock {
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub parent_hash: String,
}

#[derive(Debug, QueryableByName)]
pub(crate) struct ConflictingBlock {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub block_number: i64,
}

pub(crate) fn from_provider_blocks(
    chain_id: &ChainId,
    blocks_by_number: &HashMap<U64, Block<TxHash>>,
) -> Vec<ChainBlock> {
    let mut blocks: Vec<_> = blocks_by_number
        .values()
        .filter_map(|block| {
            let block_number = block.number?;
            let block_hash = block.hash?;

            Some(ChainBlock {
                chain_id: *chain_id as i64,
                block_number: block_number.as_u64() as i64,
                block_hash: h256_to_string(&block_hash),
                parent_hash: h256_to_string(&block.parent_hash),
            })
        })
        .collect();

    blocks.sort_by_key(|block| block.block_number);
    blocks.dedup_by_key(|block| block.block_number);
    blocks
}

pub(crate) fn earliest_conflicting_block_query(blocks: &[ChainBlock]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    Some(format!(
        "WITH incoming(chain_id, block_number, block_hash, parent_hash, status) AS (VALUES {})
         SELECT existing.block_number
         FROM chaindexing_blocks existing
         JOIN incoming
           ON incoming.chain_id = existing.chain_id
          AND incoming.block_number = existing.block_number
         WHERE existing.status = 'canonical'
           AND existing.block_hash <> incoming.block_hash
         ORDER BY existing.block_number ASC
         LIMIT 1",
        values(blocks)
    ))
}

pub(crate) fn mark_reorged_from_query(chain_id: ChainId, block_number: i64) -> String {
    format!(
        "UPDATE chaindexing_blocks
         SET status = 'reorged'
         WHERE chain_id = {}
           AND block_number >= {}
           AND status = 'canonical'",
        chain_id, block_number
    )
}

pub(crate) fn upsert_blocks_query(blocks: &[ChainBlock]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    Some(format!(
        "INSERT INTO chaindexing_blocks
            (chain_id, block_number, block_hash, parent_hash, status)
         VALUES {}
         ON CONFLICT (chain_id, block_number, block_hash)
         DO UPDATE SET
            parent_hash = EXCLUDED.parent_hash,
            status = EXCLUDED.status",
        values(blocks)
    ))
}

fn values(blocks: &[ChainBlock]) -> String {
    blocks
        .iter()
        .map(|block| {
            format!(
                "({}, {}, '{}', '{}', 'canonical')",
                block.chain_id,
                block.block_number,
                escape_sql_literal(&block.block_hash),
                escape_sql_literal(&block.parent_hash)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn h256_to_string(h256: &H256) -> String {
    serde_json::to_value(h256).unwrap().as_str().unwrap().to_lowercase()
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h256(value: u64) -> H256 {
        H256::from_low_u64_be(value)
    }

    #[test]
    fn returns_none_queries_for_empty_block_lists() {
        assert_eq!(earliest_conflicting_block_query(&[]), None);
        assert_eq!(upsert_blocks_query(&[]), None);
    }

    #[test]
    fn builds_conflict_query_for_canonical_blocks() {
        let blocks = vec![ChainBlock {
            chain_id: 1,
            block_number: 10,
            block_hash: "0xabc".to_string(),
            parent_hash: "0xdef".to_string(),
        }];

        let query = earliest_conflicting_block_query(&blocks).unwrap();

        assert!(query.contains("(1, 10, '0xabc', '0xdef', 'canonical')"));
        assert!(query.contains("existing.status = 'canonical'"));
        assert!(query.contains("existing.block_hash <> incoming.block_hash"));
    }

    #[test]
    fn extracts_provider_blocks_with_hashes() {
        let mut blocks_by_number = HashMap::new();
        blocks_by_number.insert(
            U64::from(10),
            Block {
                number: Some(U64::from(10)),
                hash: Some(h256(2)),
                parent_hash: h256(1),
                ..Default::default()
            },
        );

        let blocks = from_provider_blocks(&ChainId::Mainnet, &blocks_by_number);

        assert_eq!(
            blocks,
            vec![ChainBlock {
                chain_id: 1,
                block_number: 10,
                block_hash: "0x0000000000000000000000000000000000000000000000000000000000000002"
                    .to_string(),
                parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000001"
                    .to_string(),
            }]
        );
    }

    #[test]
    fn skips_provider_blocks_without_hashes() {
        let mut blocks_by_number = HashMap::new();
        blocks_by_number.insert(
            U64::from(10),
            Block {
                number: Some(U64::from(10)),
                ..Default::default()
            },
        );

        assert!(from_provider_blocks(&ChainId::Mainnet, &blocks_by_number).is_empty());
    }
}
