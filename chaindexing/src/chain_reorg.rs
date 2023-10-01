use std::cmp::max;

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

pub enum Execution<'a> {
    Main,
    Confirmation(&'a MinConfirmationCount),
}
