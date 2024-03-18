use crate::diesels::schema::chaindexing_reset_counts;
use diesel::{Insertable, Queryable};

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_reset_counts)]
pub struct ResetCount {
    id: i64,
    inserted_at: chrono::NaiveDateTime,
}

impl ResetCount {
    pub fn get_count(&self) -> u64 {
        self.id as u64
    }
}

pub const MAX_RESET_COUNT: u64 = 1_000;
