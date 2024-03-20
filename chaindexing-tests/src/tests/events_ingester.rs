#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::factory::{
        bayc_contract, empty_json_rpc, BAYC_CONTRACT_ADDRESS, BAYC_CONTRACT_START_BLOCK_NUMBER,
    };
    use crate::{
        json_rpc_with_empty_logs, json_rpc_with_filter_stubber, json_rpc_with_logs, test_runner,
    };
    use chaindexing::{
        ChainId, ChaindexingRepo, Contract, EventsIngester, HasRawQueryClient,
        MinConfirmationCount, Repo,
    };

    #[tokio::test]
    pub async fn creates_contract_events() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let bayc_contract = bayc_contract();
            let contracts = vec![bayc_contract.clone()];

            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let json_rpc = Arc::new(json_rpc_with_logs!(
                BAYC_CONTRACT_ADDRESS,
                CURRENT_BLOCK_NUMBER
            ));

            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
            ChaindexingRepo::create_contract_addresses(&mut conn, &bayc_contract.addresses).await;

            let conn = Arc::new(Mutex::new(conn));
            let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
            EventsIngester::ingest(
                conn.clone(),
                &raw_query_client,
                &contracts,
                10,
                json_rpc,
                &ChainId::Mainnet,
                &MinConfirmationCount::new(1),
                10,
                0,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let mut conn = conn.lock().await;
            let ingested_events = ChaindexingRepo::get_all_events(&mut conn).await;
            let first_event = ingested_events.first().unwrap();
            assert_eq!(
                first_event.contract_address,
                BAYC_CONTRACT_ADDRESS.to_lowercase()
            );
        })
        .await;
    }

    #[tokio::test]
    pub async fn starts_from_start_block_number() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let bayc_contract = bayc_contract();
            let contracts = vec![bayc_contract.clone()];

            ChaindexingRepo::create_contract_addresses(&mut conn, &bayc_contract.addresses).await;
            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let bayc_contract_address = contract_addresses.first().unwrap();
            assert_eq!(
                bayc_contract_address.next_block_number_to_ingest_from as u32,
                BAYC_CONTRACT_START_BLOCK_NUMBER
            );
            let json_rpc = Arc::new(json_rpc_with_filter_stubber!(
                BAYC_CONTRACT_ADDRESS,
                |filter: &Filter| {
                    assert_eq!(
                        filter.get_from_block().unwrap().as_u32(),
                        BAYC_CONTRACT_START_BLOCK_NUMBER
                    );
                }
            ));

            let conn = Arc::new(Mutex::new(conn));
            let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
            EventsIngester::ingest(
                conn.clone(),
                &raw_query_client,
                &contracts,
                10,
                json_rpc,
                &ChainId::Mainnet,
                &MinConfirmationCount::new(1),
                10,
                0,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
        })
        .await;
    }

    #[tokio::test]
    pub async fn updates_next_block_number_to_ingest_from_for_a_given_batch() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let bayc_contract = bayc_contract();
            let contracts = vec![bayc_contract.clone()];

            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let json_rpc = Arc::new(json_rpc_with_logs!(
                BAYC_CONTRACT_ADDRESS,
                CURRENT_BLOCK_NUMBER
            ));

            ChaindexingRepo::create_contract_addresses(&mut conn, &bayc_contract.addresses).await;

            let conn = Arc::new(Mutex::new(conn));
            let blocks_per_batch = 10;

            let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
            EventsIngester::ingest(
                conn.clone(),
                &raw_query_client,
                &contracts,
                blocks_per_batch,
                json_rpc,
                &ChainId::Mainnet,
                &MinConfirmationCount::new(1),
                10,
                0,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let mut conn = conn.lock().await;
            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let bayc_contract_address = contract_addresses.first().unwrap();
            let next_block_number_to_ingest_from =
                bayc_contract_address.next_block_number_to_ingest_from as u64;
            assert_eq!(
                next_block_number_to_ingest_from,
                BAYC_CONTRACT_START_BLOCK_NUMBER as u64 + blocks_per_batch + 1
            );
        })
        .await;
    }

    // TODO:
    #[tokio::test]
    pub async fn continues_from_next_block_number_to_ingest_from() {}

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let contracts: Vec<Contract<()>> = vec![];
            let json_rpc = Arc::new(empty_json_rpc());
            let blocks_per_batch = 10;
            let conn = Arc::new(Mutex::new(conn));
            let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
            EventsIngester::ingest(
                conn.clone(),
                &raw_query_client,
                &contracts,
                blocks_per_batch,
                json_rpc,
                &ChainId::Mainnet,
                &MinConfirmationCount::new(1),
                10,
                0,
                &mut HashMap::new(),
            )
            .await
            .unwrap();
            let mut conn = conn.lock().await;
            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_events_from_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let bayc_contract = bayc_contract();
            let contracts = vec![bayc_contract.clone()];
            let json_rpc = Arc::new(json_rpc_with_empty_logs!(BAYC_CONTRACT_ADDRESS));

            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
            ChaindexingRepo::create_contract_addresses(&mut conn, &bayc_contract.addresses).await;

            let conn = Arc::new(Mutex::new(conn));
            let raw_query_client = test_runner::new_repo().get_raw_query_client().await;
            EventsIngester::ingest(
                conn.clone(),
                &raw_query_client,
                &contracts,
                10,
                json_rpc,
                &ChainId::Mainnet,
                &MinConfirmationCount::new(1),
                10,
                0,
                &mut HashMap::new(),
            )
            .await
            .unwrap();

            let mut conn = conn.lock().await;
            assert!(ChaindexingRepo::get_all_events(&mut conn).await.is_empty());
        })
        .await;
    }
}
