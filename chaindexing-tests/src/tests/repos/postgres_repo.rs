#[cfg(test)]
mod create_initial_contract_addresses {
    use chaindexing::{Chain, ChaindexingRepo, Repo, UnsavedContractAddress};

    use crate::test_runner;

    #[tokio::test]
    pub async fn creates_contract_addresses() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contract_name = "Test-contract-address";
            let contract_address_value = "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e";
            let chain = Chain::Arbitrum;
            let start_block_number = 0;

            let contract_addresses = vec![UnsavedContractAddress::new(
                contract_name,
                contract_address_value,
                &chain,
                start_block_number,
            )];
            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let contract_address = contract_addresses.first().unwrap();

            assert_eq!(contract_address.contract_name, contract_name);
            assert_eq!(
                contract_address.address,
                contract_address_value.to_lowercase()
            );
            assert_eq!(contract_address.start_block_number, start_block_number);
        })
        .await;
    }

    #[tokio::test]
    pub async fn sets_next_block_number_to_ingest_from_with_provided_start_block_number() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contract_name = "Test-contract-address";
            let contract_address_value = "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e";
            let chain = Chain::Arbitrum;
            let start_block_number = 0;

            let contract_addresses = vec![UnsavedContractAddress::new(
                contract_name,
                contract_address_value,
                &chain,
                start_block_number,
            )];
            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let contract_address = contract_addresses.first().unwrap();

            assert_eq!(
                contract_address.next_block_number_to_ingest_from,
                start_block_number
            );
        })
        .await;
    }

    #[tokio::test]
    pub async fn sets_next_block_number_to_handle_from_with_provided_start_block_number() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let contract_name = "Test-contract-address";
            let contract_address_value = "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e";
            let chain = Chain::Arbitrum;
            let start_block_number = 0;

            let contract_addresses = vec![UnsavedContractAddress::new(
                contract_name,
                contract_address_value,
                &chain,
                start_block_number,
            )];
            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let contract_address = contract_addresses.first().unwrap();

            assert_eq!(
                contract_address.next_block_number_to_handle_from,
                start_block_number
            );
        })
        .await;
    }

    #[tokio::test]
    pub async fn overwrites_contract_name_of_contract_addresses() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let initial_contract_address = UnsavedContractAddress::new(
                "initial-contract-address",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                &Chain::Arbitrum,
                0,
            );

            let contract_addresses = vec![initial_contract_address];
            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let updated_contract_address = UnsavedContractAddress::new(
                "updated-contract-address",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                &Chain::Arbitrum,
                0,
            );
            let contract_addresses = vec![updated_contract_address];

            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let contract_address = contract_addresses.first().unwrap();

            assert_eq!(contract_address.contract_name, "updated-contract-address");
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_not_update_block_number_related_fields() {
        let pool = test_runner::get_pool().await;

        test_runner::run_test(&pool, |mut conn| async move {
            let initial_start_block_number = 400;

            let initial_contract_address = UnsavedContractAddress::new(
                "initial-contract-address",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                &Chain::Arbitrum,
                initial_start_block_number,
            );

            let contract_addresses = vec![initial_contract_address];
            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let updated_contract_address = UnsavedContractAddress::new(
                "updated-contract-address",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                &Chain::Arbitrum,
                2000,
            );
            let contract_addresses = vec![updated_contract_address];

            ChaindexingRepo::create_contract_addresses(&mut conn, &contract_addresses).await;

            let contract_addresses = ChaindexingRepo::get_all_contract_addresses(&mut conn).await;
            let contract_address = contract_addresses.first().unwrap();

            assert_eq!(
                contract_address.start_block_number,
                initial_start_block_number
            );
            assert_eq!(
                contract_address.next_block_number_to_ingest_from,
                initial_start_block_number
            );
            assert_eq!(
                contract_address.next_block_number_to_handle_from,
                initial_start_block_number
            );
        })
        .await;
    }
}
