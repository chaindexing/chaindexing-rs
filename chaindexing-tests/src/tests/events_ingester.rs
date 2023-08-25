#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{factory, test_runner};
    use chaindexing::{Chain, Chaindexing, Contract, EventsIngester, PostgresRepo, Repo};
    use tokio::sync::Mutex;

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_contracts() {
        let pool = test_runner::get_single_pool().await;

        test_runner::run_test(
            &pool,
            |conn| async move {
                let conn = Arc::new(Mutex::new(conn));

                let json_rpc = Arc::new(factory::empty_json_rpc());

                let contracts = vec![
                Contract::new("BoredApeYachtClub")
                .add_event("event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)", factory::TestEventHandler)
                .add_address(
                    "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
                    &Chain::Mainnet,
                    17773490,
                )
            ];

                let blocks_per_batch = 10;

                EventsIngester::ingest(conn.clone(), &contracts, blocks_per_batch, json_rpc)
                    .await
                    .unwrap();

                let mut conn = conn.lock().await;

                Chaindexing::create_initial_contract_addresses(&mut conn, &contracts).await;

                let _contract_addresses = PostgresRepo::get_all_contract_addresses(&mut conn).await;

                // assert_eq!(contract_addresses, vec![]);

                let all_events = PostgresRepo::get_all_events(&mut conn).await;
                assert_eq!(all_events, vec![]);
            },
        ).await;
    }

    #[tokio::test]
    pub async fn creates_events_of_interest_from_contracts() {}

    #[tokio::test]
    pub async fn checkpoints_contract_addresses_last_ingested_block_number() {}

    #[tokio::test]
    pub async fn starts_from_start_block_number() {}

    #[tokio::test]
    pub async fn continues_from_last_ingested_block_number() {}

    #[tokio::test]
    pub async fn does_nothing_when_there_are_no_events_from_contracts() {}
}
