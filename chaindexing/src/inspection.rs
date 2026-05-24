#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct InspectionQuery {
    pub chain_id: u64,
    pub from_block_number: u64,
    pub limit: u32,
}

impl InspectionQuery {
    pub fn new(chain_id: u64) -> Self {
        Self {
            chain_id,
            from_block_number: 0,
            limit: 100,
        }
    }

    pub fn from_block_number(mut self, from_block_number: u64) -> Self {
        self.from_block_number = from_block_number;
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit.max(1);
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InspectionQueries;

impl InspectionQueries {
    pub fn canonical_events(query: InspectionQuery) -> String {
        format!(
            "SELECT *
             FROM chaindexing_events
             WHERE chain_id = {}
               AND block_number >= {}
               AND status = 'canonical'
             ORDER BY block_number ASC, transaction_index ASC, log_index ASC
             LIMIT {}",
            query.chain_id, query.from_block_number, query.limit
        )
    }

    pub fn canonical_transactions(query: InspectionQuery) -> String {
        format!(
            "SELECT *
             FROM chaindexing_transactions
             WHERE chain_id = {}
               AND block_number >= {}
               AND status = 'canonical'
             ORDER BY block_number ASC, transaction_index ASC
             LIMIT {}",
            query.chain_id, query.from_block_number, query.limit
        )
    }

    pub fn canonical_call_traces(query: InspectionQuery) -> String {
        format!(
            "SELECT *
             FROM chaindexing_call_traces
             WHERE chain_id = {}
               AND block_number >= {}
               AND status = 'canonical'
             ORDER BY block_number ASC, transaction_hash ASC, trace_index ASC
             LIMIT {}",
            query.chain_id, query.from_block_number, query.limit
        )
    }

    pub fn canonical_blocks(query: InspectionQuery) -> String {
        format!(
            "SELECT *
             FROM chaindexing_blocks
             WHERE chain_id = {}
               AND block_number >= {}
               AND status = 'canonical'
             ORDER BY block_number ASC
             LIMIT {}",
            query.chain_id, query.from_block_number, query.limit
        )
    }

    pub fn recent_reorgs(chain_id: u64, limit: u32) -> String {
        format!(
            "SELECT *
             FROM chaindexing_reorgs
             WHERE chain_id = {}
             ORDER BY id DESC
             LIMIT {}",
            chain_id,
            limit.max(1)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_query_defaults_to_first_hundred_rows() {
        let query = InspectionQuery::new(1);

        assert_eq!(query.chain_id, 1);
        assert_eq!(query.from_block_number, 0);
        assert_eq!(query.limit, 100);
    }

    #[test]
    fn inspection_queries_are_status_scoped_and_ordered() {
        let query = InspectionQuery::new(1).from_block_number(42).limit(10);
        let transactions = InspectionQueries::canonical_transactions(query);
        let traces = InspectionQueries::canonical_call_traces(query);

        assert!(transactions.contains("FROM chaindexing_transactions"));
        assert!(transactions.contains("status = 'canonical'"));
        assert!(transactions.contains("block_number >= 42"));
        assert!(transactions.contains("LIMIT 10"));
        assert!(traces.contains("ORDER BY block_number ASC, transaction_hash ASC, trace_index ASC"));
    }

    #[test]
    fn inspection_limit_never_builds_zero_limit_queries() {
        let query = InspectionQuery::new(1).limit(0);
        let events = InspectionQueries::canonical_events(query);
        let reorgs = InspectionQueries::recent_reorgs(1, 0);

        assert!(events.contains("LIMIT 1"));
        assert!(reorgs.contains("LIMIT 1"));
    }
}
