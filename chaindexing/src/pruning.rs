pub fn get_min_block_number(prune_n_blocks_away: u64, current_block_number: u64) -> u64 {
    if current_block_number < prune_n_blocks_away {
        current_block_number
    } else {
        current_block_number - prune_n_blocks_away
    }
}
