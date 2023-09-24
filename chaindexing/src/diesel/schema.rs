// @generated automatically by Diesel CLI.

diesel::table! {
  chaindexing_contract_addresses (id) {
      id -> Int4,
      chain_id -> Int4,
      next_block_number_to_ingest_from -> Int8,
      next_block_number_to_handle_from -> Int8,
      start_block_number -> Int8,
      address -> Text,
      contract_name -> Text,
  }
}

diesel::table! {
  chaindexing_events (id) {
      id -> Uuid,
      contract_address -> Text,
      contract_name -> Text,
      abi -> Text,
      log_params -> Json,
      parameters -> Json,
      topics -> Json,
      block_hash -> Text,
      block_number -> Int8,
      transaction_hash -> Text,
      transaction_index -> Int8,
      log_index -> Int8,
      removed -> Bool,
      inserted_at -> Timestamptz,
  }
}

diesel::allow_tables_to_appear_in_same_query!(chaindexing_contract_addresses, chaindexing_events,);
