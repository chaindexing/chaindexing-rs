use crate::{InspectionQueries, InspectionQuery};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InspectionResource {
    Events,
    Blocks,
    Transactions,
    CallTraces,
    Reorgs,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct InspectionUiQuery {
    pub chain_id: u64,
    pub from_block_number: u64,
    pub limit: u32,
}

impl InspectionUiQuery {
    pub const MAX_LIMIT: u32 = 1_000;

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
        self.limit = limit.clamp(1, Self::MAX_LIMIT);
        self
    }

    fn inspection_query(self) -> InspectionQuery {
        InspectionQuery::new(self.chain_id)
            .from_block_number(self.from_block_number)
            .limit(self.limit)
    }
}

pub struct InspectionUi;

impl InspectionUi {
    pub fn html() -> &'static str {
        include_str!("inspection_ui.html")
    }

    pub fn query(resource: InspectionResource, query: InspectionUiQuery) -> String {
        match resource {
            InspectionResource::Events => {
                InspectionQueries::canonical_events(query.inspection_query())
            }
            InspectionResource::Blocks => {
                InspectionQueries::canonical_blocks(query.inspection_query())
            }
            InspectionResource::Transactions => {
                InspectionQueries::canonical_transactions(query.inspection_query())
            }
            InspectionResource::CallTraces => {
                InspectionQueries::canonical_call_traces(query.inspection_query())
            }
            InspectionResource::Reorgs => {
                InspectionQueries::recent_reorgs(query.chain_id, query.limit)
            }
        }
    }

    pub fn resource_from_api_path(path: &str) -> Option<InspectionResource> {
        match path {
            "/api/events" => Some(InspectionResource::Events),
            "/api/blocks" => Some(InspectionResource::Blocks),
            "/api/transactions" => Some(InspectionResource::Transactions),
            "/api/call-traces" => Some(InspectionResource::CallTraces),
            "/api/reorgs" => Some(InspectionResource::Reorgs),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_ui_maps_api_paths_to_resources() {
        assert_eq!(
            InspectionUi::resource_from_api_path("/api/events"),
            Some(InspectionResource::Events)
        );
        assert_eq!(
            InspectionUi::resource_from_api_path("/api/call-traces"),
            Some(InspectionResource::CallTraces)
        );
        assert_eq!(InspectionUi::resource_from_api_path("/api/unknown"), None);
    }

    #[test]
    fn inspection_ui_queries_are_read_only_and_status_scoped() {
        let query = InspectionUiQuery::new(1).from_block_number(42).limit(10);
        let sql = InspectionUi::query(InspectionResource::Transactions, query);

        assert!(sql.trim_start().starts_with("SELECT"));
        assert!(sql.contains("FROM chaindexing_transactions"));
        assert!(sql.contains("status = 'canonical'"));
        assert!(sql.contains("block_number >= 42"));
        assert!(sql.contains("LIMIT 10"));
    }

    #[test]
    fn inspection_ui_clamps_expensive_limits() {
        let query = InspectionUiQuery::new(1).limit(10_000);
        let sql = InspectionUi::query(InspectionResource::Events, query);

        assert!(sql.contains("LIMIT 1000"));
    }

    #[test]
    fn inspection_ui_html_contains_api_resources() {
        let html = InspectionUi::html();

        for resource in ["events", "blocks", "transactions", "call-traces", "reorgs"] {
            assert!(html.contains(resource));
        }
    }
}
