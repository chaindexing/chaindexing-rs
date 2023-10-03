use std::{cmp::max, collections::HashMap};

use crate::diesels::schema::chaindexing_reorged_blocks;
use diesel::prelude::{Insertable, Queryable};

use ethers::types::Chain;

/// Tolerance for chain re-organization
#[derive(Clone)]
pub struct MinConfirmationCount {
    value: u8,
}

impl MinConfirmationCount {
    pub fn new(value: u8) -> Self {
        Self { value }
    }

    pub fn ideduct_from(&self, block_number: i64, start_block_number: i64) -> u64 {
        self.deduct_from(block_number as u64, start_block_number as u64)
    }

    pub fn deduct_from(&self, block_number: u64, start_block_number: u64) -> u64 {
        max(start_block_number, block_number - (self.value as u64))
    }
}

#[derive(Clone)]
pub enum Execution<'a> {
    Main,
    Confirmation(&'a MinConfirmationCount),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Queryable)]
#[diesel(table_name = chaindexing_reorged_blocks)]
pub struct ReorgedBlock {
    pub id: i32,
    pub block_number: i64,
    pub chain_id: i32,
    handled_at: Option<chrono::NaiveDateTime>,
    inserted_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = chaindexing_reorged_blocks)]
pub struct UnsavedReorgedBlock {
    pub block_number: i64,
    pub chain_id: i32,
    handled_at: Option<chrono::NaiveDateTime>,
    inserted_at: chrono::NaiveDateTime,
}

impl UnsavedReorgedBlock {
    pub fn new(block_number: i64, chain: &Chain) -> Self {
        Self {
            block_number,
            chain_id: *chain as i32,
            handled_at: None,
            inserted_at: chrono::Utc::now().naive_utc(),
        }
    }
}

pub struct ReorgedBlocks;

impl ReorgedBlocks {
    pub fn only_earliest_per_chain(reorged_blocks: &Vec<ReorgedBlock>) -> Vec<ReorgedBlock> {
        reorged_blocks
            .into_iter()
            .fold(
                HashMap::<i32, ReorgedBlock>::new(),
                |mut reorged_blocks_by_chain, reorged_block| {
                    let ReorgedBlock { chain_id, .. } = reorged_block;

                    if let Some(earliest_reorged_block) = reorged_blocks_by_chain.get(chain_id) {
                        if reorged_block.block_number < (*earliest_reorged_block).block_number {
                            reorged_blocks_by_chain.insert(*chain_id, reorged_block.clone());
                        }
                    } else {
                        reorged_blocks_by_chain
                            .insert(reorged_block.chain_id, reorged_block.clone());
                    }

                    reorged_blocks_by_chain
                },
            )
            .values()
            .cloned()
            .collect()
    }

    pub fn get_ids(reorged_blocks: &Vec<ReorgedBlock>) -> Vec<i32> {
        reorged_blocks.iter().map(|r| r.id).collect()
    }
}
