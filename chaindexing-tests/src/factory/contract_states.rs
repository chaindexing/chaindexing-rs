#[derive(Clone, Debug, Serialize, Deserialize)]
struct NftState {
    token_id: i32,
    contract_address: String,
    owner_address: String,
}

impl ContractState for NftState {
    fn table_name() -> &'static str {
        "nft_states"
    }
}

struct NftStateMigrations;

impl ContractStateMigrations for NftStateMigrations {
    fn migrations(&self) -> Vec<&'static str> {
        vec![
            "CREATE TABLE IF NOT EXISTS nft_states (
                token_id INTEGER NOT NULL,
                contract_address TEXT NOT NULL,
                owner_address TEXT NOT NULL
            )",
        ]
    }
}
