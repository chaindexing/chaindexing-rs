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
pub(crate) struct CanonicalBlock {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub block_number: i64,
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub block_hash: String,
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

pub(crate) fn canonical_blocks_query(blocks: &[ChainBlock]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    let chain_id = blocks[0].chain_id;
    let block_numbers = blocks
        .iter()
        .map(|block| block.block_number.to_string())
        .collect::<Vec<_>>()
        .join(",");

    Some(format!(
        "SELECT block_number, block_hash::TEXT AS block_hash
         FROM chaindexing_blocks
         WHERE chain_id = {chain_id}
           AND status = 'canonical'
           AND block_number IN ({block_numbers})
         ORDER BY block_number ASC"
    ))
}

pub(crate) fn find_fork_point(
    incoming_blocks: &[ChainBlock],
    canonical_blocks: &[CanonicalBlock],
) -> Option<i64> {
    let incoming_by_number: HashMap<_, _> =
        incoming_blocks.iter().map(|block| (block.block_number, block)).collect();
    let canonical_by_number: HashMap<_, _> = canonical_blocks
        .iter()
        .map(|block| (block.block_number, block.block_hash.as_str()))
        .collect();

    let mut fork_point = incoming_blocks
        .iter()
        .filter(|incoming_block| {
            canonical_by_number
                .get(&incoming_block.block_number)
                .map(|canonical_hash| *canonical_hash != incoming_block.block_hash)
                .unwrap_or(false)
        })
        .map(|block| block.block_number)
        .min()?;

    while let Some(incoming_block) = incoming_by_number.get(&fork_point) {
        let parent_block_number = fork_point - 1;
        let Some(canonical_parent_hash) = canonical_by_number.get(&parent_block_number) else {
            break;
        };

        if *canonical_parent_hash == incoming_block.parent_hash {
            break;
        }

        if incoming_by_number.contains_key(&parent_block_number) {
            fork_point = parent_block_number;
        } else {
            break;
        }
    }

    Some(fork_point)
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
        assert_eq!(canonical_blocks_query(&[]), None);
        assert_eq!(upsert_blocks_query(&[]), None);
    }

    #[test]
    fn builds_canonical_blocks_query() {
        let blocks = vec![
            ChainBlock {
                chain_id: 1,
                block_number: 10,
                block_hash: "0xaaa".to_string(),
                parent_hash: "0x999".to_string(),
            },
            ChainBlock {
                chain_id: 1,
                block_number: 11,
                block_hash: "0xbbb".to_string(),
                parent_hash: "0xaaa".to_string(),
            },
        ];

        let query = canonical_blocks_query(&blocks).unwrap();

        assert!(query.contains("chain_id = 1"));
        assert!(query.contains("block_number IN (10,11)"));
        assert!(query.contains("status = 'canonical'"));
    }

    #[test]
    fn finds_first_conflicting_block_when_parent_matches() {
        let incoming = vec![
            ChainBlock {
                chain_id: 1,
                block_number: 10,
                block_hash: "0xold10".to_string(),
                parent_hash: "0xold9".to_string(),
            },
            ChainBlock {
                chain_id: 1,
                block_number: 11,
                block_hash: "0xnew11".to_string(),
                parent_hash: "0xold10".to_string(),
            },
        ];
        let canonical = vec![
            CanonicalBlock {
                block_number: 10,
                block_hash: "0xold10".to_string(),
            },
            CanonicalBlock {
                block_number: 11,
                block_hash: "0xold11".to_string(),
            },
        ];

        assert_eq!(find_fork_point(&incoming, &canonical), Some(11));
    }

    #[test]
    fn walks_back_to_true_fork_point_when_parent_mismatches() {
        let incoming = vec![
            ChainBlock {
                chain_id: 1,
                block_number: 9,
                block_hash: "0xold9".to_string(),
                parent_hash: "0xold8".to_string(),
            },
            ChainBlock {
                chain_id: 1,
                block_number: 10,
                block_hash: "0xnew10".to_string(),
                parent_hash: "0xold9".to_string(),
            },
            ChainBlock {
                chain_id: 1,
                block_number: 11,
                block_hash: "0xnew11".to_string(),
                parent_hash: "0xnew10".to_string(),
            },
        ];
        let canonical = vec![
            CanonicalBlock {
                block_number: 9,
                block_hash: "0xold9".to_string(),
            },
            CanonicalBlock {
                block_number: 10,
                block_hash: "0xold10".to_string(),
            },
            CanonicalBlock {
                block_number: 11,
                block_hash: "0xold11".to_string(),
            },
        ];

        assert_eq!(find_fork_point(&incoming, &canonical), Some(10));
    }

    #[test]
    fn returns_none_when_incoming_blocks_match_canonical_blocks() {
        let incoming = vec![ChainBlock {
            chain_id: 1,
            block_number: 10,
            block_hash: "0xold10".to_string(),
            parent_hash: "0xold9".to_string(),
        }];
        let canonical = vec![CanonicalBlock {
            block_number: 10,
            block_hash: "0xold10".to_string(),
        }];

        assert_eq!(find_fork_point(&incoming, &canonical), None);
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
