use std::sync::Arc;

use futures_util::FutureExt;

use crate::chain_reorg::Execution;
use crate::contracts::Contract;
use crate::events::Events;
use crate::{ChaindexingRepo, ChaindexingRepoConn, ContractAddress, EventsIngesterJsonRpc, Repo};

use super::{fetch_blocks_by_tx_hash, fetch_logs, EventsIngesterError, Filter, Filters};

pub struct IngestEvents;

impl IngestEvents {
    pub async fn run<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: Vec<ContractAddress>,
        contracts: &Vec<Contract>,
        json_rpc: &Arc<impl EventsIngesterJsonRpc + 'static>,
        current_block_number: u64,
        blocks_per_batch: u64,
    ) -> Result<(), EventsIngesterError> {
        let filters = Filters::new(
            &contract_addresses,
            &contracts,
            current_block_number,
            blocks_per_batch,
            &Execution::Main,
        );

        if !filters.is_empty() {
            let logs = fetch_logs(&filters, json_rpc).await;
            let blocks_by_tx_hash = fetch_blocks_by_tx_hash(&logs, json_rpc).await;
            let events = Events::new(&logs, &contracts, &blocks_by_tx_hash);

            ChaindexingRepo::run_in_transaction(conn, move |conn| {
                async move {
                    ChaindexingRepo::create_events(conn, &events.clone()).await;

                    Self::update_next_block_numbers_to_ingest_from(
                        conn,
                        &contract_addresses,
                        &filters,
                    )
                    .await;

                    Ok(())
                }
                .boxed()
            })
            .await?;
        }

        Ok(())
    }

    async fn update_next_block_numbers_to_ingest_from<'a>(
        conn: &mut ChaindexingRepoConn<'a>,
        contract_addresses: &Vec<ContractAddress>,
        filters: &Vec<Filter>,
    ) {
        let filters_by_contract_address_id = Filters::group_by_contract_address_id(filters);

        for contract_address in contract_addresses {
            let filters = filters_by_contract_address_id.get(&contract_address.id).unwrap();

            if let Some(latest_filter) = Filters::get_latest(filters) {
                let next_block_number_to_ingest_from =
                    latest_filter.value.get_to_block().unwrap() + 1;

                ChaindexingRepo::update_next_block_number_to_ingest_from(
                    conn,
                    &contract_address,
                    next_block_number_to_ingest_from.as_u64() as i64,
                )
                .await
            }
        }
    }
}
