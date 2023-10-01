use crate::diesels::schema::chaindexing_reset_counts;
use diesel::{Insertable, Queryable};

#[derive(Debug, Clone, PartialEq, Queryable, Insertable)]
#[diesel(table_name = chaindexing_reset_counts)]
pub struct ResetCount {
    id: i32,
    inserted_at: chrono::NaiveDateTime,
}
