pub mod schema {
    // @generated automatically by Diesel CLI.

    diesel::table! {
      chaindexing_nodes(id) {
          id -> Int4,
          last_active_at -> Int8,
          inserted_at -> Int8,
      }
    }

    diesel::table! {
      chaindexing_contract_addresses (id) {
          id -> Int4,
          chain_id -> Int8,
          next_block_number_to_ingest_from -> Int8,
          next_block_number_to_handle_from -> Int8,
          start_block_number -> Int8,
          address -> VarChar,
          contract_name -> VarChar,
      }
    }

    diesel::table! {
      chaindexing_events (id) {
          id -> Uuid,
          chain_id -> Int8,
          contract_address -> VarChar,
          contract_name -> VarChar,
          abi -> Text,
          log_params -> Json,
          parameters -> Json,
          topics -> Json,
          block_hash -> VarChar,
          block_number -> Int8,
          block_timestamp -> Int8,
          transaction_hash -> VarChar,
          transaction_index -> Int4,
          log_index -> Int4,
          removed -> Bool,
          inserted_at -> Timestamptz,
      }
    }

    diesel::table! {
      chaindexing_reset_counts (id) {
          id -> Int8,
          inserted_at -> Timestamptz,
      }
    }

    diesel::table! {
      chaindexing_reorged_blocks (id) {
          id -> Int4,
          block_number -> Int8,
          chain_id -> Int8,
          handled_at -> Nullable<Timestamptz>,
          inserted_at -> Timestamptz,
      }
    }

    diesel::allow_tables_to_appear_in_same_query!(
        chaindexing_contract_addresses,
        chaindexing_events,
    );
}
