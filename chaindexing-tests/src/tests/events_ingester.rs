#[cfg(test)]
mod tests {
    use ethers::types::U64;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    use crate::factory::{
        bayc_contract, empty_json_rpc, BAYC_CONTRACT_ADDRESS, BAYC_CONTRACT_START_BLOCK_NUMBER,
    };
    use crate::{
        json_rpc_with_empty_logs, json_rpc_with_filter_stubber, json_rpc_with_logs, test_runner,
    };
    use chaindexing::{Chaindexing, EventsIngester, PostgresRepo, Repo};

    #[tokio::test]
    pub async fn creates_contract_events() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contracts = vec![bayc_contract()];
            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let json_rpc = Arc::new(json_rpc_with_logs!(
                BAYC_CONTRACT_ADDRESS,
                CURRENT_BLOCK_NUMBER
            ));

            assert!(PostgresRepo::get_all_events(&mut conn).await.is_empty());
            Chaindexing::create_initial_contract_addresses(&mut conn, &contracts).await;

            let conn = Arc::new(Mutex::new(conn));
            EventsIngester::ingest(conn.clone(), &contracts, 10, json_rpc)
                .await
                .unwrap();

            let mut conn = conn.lock().await;
            let ingested_events = PostgresRepo::get_all_events(&mut conn).await;
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
            let contracts = vec![bayc_contract()];

            Chaindexing::create_initial_contract_addresses(&mut conn, &contracts).await;
            let contract_addresses = PostgresRepo::get_all_contract_addresses(&mut conn).await;
            let bayc_contract_address = contract_addresses.first().unwrap();
            assert_eq!(
                bayc_contract_address.last_ingested_block_number as u32,
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
            EventsIngester::ingest(conn, &contracts, 10, json_rpc)
                .await
                .unwrap();
        })
        .await;
    }

    #[tokio::test]
    pub async fn checkpoints_last_ingested_block_to_the_ingested_block_in_a_given_batch() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contracts = vec![bayc_contract()];
            static CURRENT_BLOCK_NUMBER: u32 = BAYC_CONTRACT_START_BLOCK_NUMBER + 20;
            let json_rpc = Arc::new(json_rpc_with_logs!(
                BAYC_CONTRACT_ADDRESS,
                CURRENT_BLOCK_NUMBER
            ));

            Chaindexing::create_initial_contract_addresses(&mut conn, &contracts).await;

            let conn = Arc::new(Mutex::new(conn));
            let blocks_per_batch = 10;
            EventsIngester::ingest(conn.clone(), &contracts, blocks_per_batch, json_rpc)
                .await
                .unwrap();

            let mut conn = conn.lock().await;
            let contract_addresses = PostgresRepo::get_all_contract_addresses(&mut conn).await;
            let bayc_contract_address = contract_addresses.first().unwrap();
            let last_ingested_block_number =
                bayc_contract_address.last_ingested_block_number as u64;
            assert_eq!(
                last_ingested_block_number,
                BAYC_CONTRACT_START_BLOCK_NUMBER as u64 + blocks_per_batch
            );
        })
        .await;
    }

    // TODO:
    #[tokio::test]
    pub async fn continues_from_last_ingested_block_number() {}

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |conn| async move {
            let contracts = vec![];
            let json_rpc = Arc::new(empty_json_rpc());
            let blocks_per_batch = 10;
            let conn = Arc::new(Mutex::new(conn));
            EventsIngester::ingest(conn.clone(), &contracts, blocks_per_batch, json_rpc)
                .await
                .unwrap();
            let mut conn = conn.lock().await;
            let all_events = PostgresRepo::get_all_events(&mut conn).await;
            assert_eq!(all_events, vec![]);
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_events_from_contracts() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contracts = vec![bayc_contract()];
            let json_rpc = Arc::new(json_rpc_with_empty_logs!(BAYC_CONTRACT_ADDRESS));

            assert!(PostgresRepo::get_all_events(&mut conn).await.is_empty());
            Chaindexing::create_initial_contract_addresses(&mut conn, &contracts).await;

            let conn = Arc::new(Mutex::new(conn));
            EventsIngester::ingest(conn.clone(), &contracts, 10, json_rpc)
                .await
                .unwrap();

            let mut conn = conn.lock().await;
            assert!(PostgresRepo::get_all_events(&mut conn).await.is_empty());
        })
        .await;
    }
}
