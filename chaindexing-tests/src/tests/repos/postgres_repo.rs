#[cfg(test)]
mod create_contract_addresses {
    use std::sync::Arc;

    use chaindexing::{ChainId, ChaindexingRepo, ExecutesWithRawQuery, UnsavedContractAddress};

    use tokio::sync::Mutex;

    use crate::{find_contract_address_by_contract_name, test_runner};

    #[tokio::test]
    pub async fn creates_contract_addresses() {
        test_runner::run_test_new(|repo_client| async move {
            let contract_name = "contract-name-1";
            let contract_address_value = "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8993e";
            let chain_id = ChainId::Arbitrum;
            let start_block_number = 0;

            let contract_addresses = vec![UnsavedContractAddress::new(
                contract_name,
                contract_address_value,
                &chain_id,
                start_block_number,
            )];
            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let repo_client = Arc::new(Mutex::new(repo_client));
            let contract_address =
                find_contract_address_by_contract_name(&repo_client, contract_name, &chain_id)
                    .await;

            assert!(contract_address.is_some());

            let contract_address = contract_address.unwrap();
            assert_eq!(
                contract_address.address,
                contract_address_value.to_lowercase()
            );
            assert_eq!(
                contract_address.start_block_number,
                start_block_number as i64
            );
        })
        .await;
    }

    #[tokio::test]
    pub async fn sets_next_block_numbers() {
        test_runner::run_test_new(|repo_client| async move {
            let contract_name = "contract-name-20";
            let contract_address_value = "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8942e";
            let chain_id = ChainId::Arbitrum;
            let start_block_number = 30;

            let contract_addresses = vec![UnsavedContractAddress::new(
                contract_name,
                contract_address_value,
                &chain_id,
                start_block_number,
            )];
            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let repo_client = Arc::new(Mutex::new(repo_client));
            let contract_address =
                find_contract_address_by_contract_name(&repo_client, contract_name, &chain_id)
                    .await
                    .unwrap();

            assert_eq!(
                contract_address.next_block_number_to_ingest_from,
                start_block_number as i64
            );
            assert_eq!(
                contract_address.next_block_number_to_handle_from,
                start_block_number as i64
            );
            assert_eq!(contract_address.next_block_number_for_side_effects, 0);
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_not_overwrite_contract_name_of_contract_addresses() {
        test_runner::run_test_new(|repo_client| async move {
            let chain_id = &ChainId::Arbitrum;
            let initial_contract_address = UnsavedContractAddress::new(
                "initial-contract-name-3",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                chain_id,
                0,
            );

            let contract_addresses = vec![initial_contract_address];
            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let updated_contract_address = UnsavedContractAddress::new(
                "updated-contract-name-3",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
                chain_id,
                0,
            );
            let contract_addresses = vec![updated_contract_address];

            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let repo_client = Arc::new(Mutex::new(repo_client));

            assert!(find_contract_address_by_contract_name(
                &repo_client,
                "initial-contract-name-3",
                chain_id
            )
            .await
            .is_some());

            assert!(find_contract_address_by_contract_name(
                &repo_client,
                "updated-contract-name-3",
                chain_id
            )
            .await
            .is_none());
        })
        .await;
    }

    #[tokio::test]
    pub async fn does_not_update_any_block_number() {
        test_runner::run_test_new(|repo_client| async move {
            let initial_start_block_number = 400;

            let chain_id = &ChainId::Arbitrum;
            let initial_contract_address = UnsavedContractAddress::new(
                "contract-name-4",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8192e",
                chain_id,
                initial_start_block_number,
            );

            let contract_addresses = vec![initial_contract_address];
            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let updated_contract_address_start_block_number = 2000;
            let updated_contract_address = UnsavedContractAddress::new(
                "contract-name-4",
                "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8192e",
                chain_id,
                updated_contract_address_start_block_number,
            );
            let contract_addresses = vec![updated_contract_address];

            ChaindexingRepo::create_contract_addresses(&repo_client, &contract_addresses).await;

            let repo_client = Arc::new(Mutex::new(repo_client));

            let contract_address =
                find_contract_address_by_contract_name(&repo_client, "contract-name-4", chain_id)
                    .await
                    .unwrap();

            assert_eq!(
                contract_address.start_block_number as u64,
                initial_start_block_number
            );
            assert_eq!(
                contract_address.next_block_number_to_handle_from as u64,
                initial_start_block_number
            );
            assert_eq!(
                contract_address.next_block_number_to_ingest_from as u64,
                initial_start_block_number
            );
            assert_eq!(
                contract_address.next_block_number_for_side_effects as u64,
                0
            );
        })
        .await;
    }
}
