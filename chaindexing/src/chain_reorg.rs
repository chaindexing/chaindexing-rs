use std::cmp::max;
use std::collections::HashMap;

use crate::diesel::schema::chaindexing_reorged_blocks;
use crate::ChainId;
use diesel::prelude::Insertable;
use serde::Deserialize;

/// Tolerance for chain re-organization
#[derive(Clone, Debug)]
pub struct MinConfirmationCount {
    value: u8,
}

impl MinConfirmationCount {
    pub fn new(value: u8) -> Self {
        Self { value }
    }

    pub fn deduct_from(&self, block_number: u64, start_block_number: u64) -> u64 {
        let deduction = max(0, (block_number as i64) - (self.value as i64));

        max(start_block_number, deduction as u64)
    }

    pub fn is_in_confirmation_window(
        &self,
        next_block_number: u64,
        current_block_number: u64,
    ) -> bool {
        if self.value as u64 >= current_block_number {
            false
        } else {
            next_block_number >= current_block_number - (self.value as u64)
        }
    }
}

#[derive(Clone)]
pub enum Execution<'a> {
    Main,
    Confirmation(&'a MinConfirmationCount),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct ReorgedBlock {
    pub id: i32,
    pub block_number: i64,
    pub chain_id: i64,
    handled_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = chaindexing_reorged_blocks)]
pub struct UnsavedReorgedBlock {
    pub block_number: i64,
    pub chain_id: i64,
}

impl UnsavedReorgedBlock {
    pub fn new(block_number: i64, chain_id: &ChainId) -> Self {
        Self {
            block_number,
            chain_id: *chain_id as i64,
        }
    }
}

pub struct ReorgedBlocks;

impl ReorgedBlocks {
    pub fn only_earliest_per_chain(reorged_blocks: &[ReorgedBlock]) -> Vec<&ReorgedBlock> {
        reorged_blocks
            .iter()
            .fold(
                HashMap::<i64, &ReorgedBlock>::new(),
                |mut reorged_blocks_by_chain, reorged_block| {
                    let ReorgedBlock { chain_id, .. } = reorged_block;

                    if let Some(earliest_reorged_block) = reorged_blocks_by_chain.get(chain_id) {
                        if reorged_block.block_number < earliest_reorged_block.block_number {
                            reorged_blocks_by_chain.insert(*chain_id, reorged_block);
                        }
                    } else {
                        reorged_blocks_by_chain.insert(reorged_block.chain_id, reorged_block);
                    }

                    reorged_blocks_by_chain
                },
            )
            .into_values()
            .collect()
    }

    pub fn get_ids<'a>(reorged_blocks: &'a [&'a ReorgedBlock]) -> Vec<i32> {
        reorged_blocks.iter().map(|r| r.id).collect()
    }
}
