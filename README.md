# Chaindexing

[<img alt="github" src="https://img.shields.io/badge/Github-jurshsmith%2Fchaindexing-blue?logo=github" height="20">](https://github.com/jurshsmith/chaindexing-rs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/chaindexing.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/chaindexing)
[<img alt="diesel-streamer build" src="https://img.shields.io/github/actions/workflow/status/jurshsmith/chaindexing-rs/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/jurshsmith/chaindexing-rs/actions?query=branch%3Amain)

A Chain Reorg-Proof indexing engine that helps aggregate states for EVM contracts in RDBMS' as accurately as possible.

Example:

Indexing states of NFTs (`NftState`) for Bored Ape Yatch Club and Doodle's contracts in a Postgres DB.

1. Setup state by specifying its RDBMS's(Postgres) table name and migration:

```rust
use serde::{Deserialize, Serialize};
use chaindexing::{ContractState, ContractStateMigrations};

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
```

2. Setup Event Handlers:

For our example, we simply need a handler for `Transfer` events.

```rust

...

use chaindexing::{Contract, EventContext, EventHandler};

struct TransferEventHandler;

#[async_trait::async_trait]
impl EventHandler for TransferEventHandler {
    async fn handle_event<'a>(&self, event_context: EventContext<'a>) {
        let event = &event_context.event;
        // Get event parameters
        let event_params = event.get_params();

        // Extract each parameter as exactly specified in the ABI:
        // "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
        let from = event_params.get("from").unwrap().clone().into_address().unwrap();
        let to = event_params.get("to").unwrap().clone().into_address().unwrap();
        let token_id = event_params.get("tokenId").unwrap().clone().into_uint().unwrap();

        if let Some(nft_state) = NftState::read_one(
            [
                ("token_id".to_owned(), token_id.to_string()),
                ("owner_address".to_owned(), from.to_string()),
            ]
            .into(),
            &event_context,
        )
        .await
        {
            let updates = [("owner_address".to_string(), to.to_string())];

            nft_state.update(updates.into(), &event_context).await;
        } else {
            NftState {
                token_id: token_id.as_u32() as i32,
                contract_address: event.contract_address.clone(),
                owner_address: to.to_string(),
            }
            .create(&event_context)
            .await;
        }
    }
}
```

3. Start the indexing background process:

```rust
...
use chaindexing::{Chain, Chaindexing, Chains, Config, Contract, PostgresRepo, Repo};

#[tokio::main]
async fn main() {
    // Setup BAYC's contract
    let bayc_contract =  Contract::new("BoredApeYachtClub")
    // add transfer event and it's corresponding handler
    .add_event("event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)", TransferEventHandler)
    // add migration for the state's DB schema
    .add_state_migrations(NftStateMigrations)
    // add contract address for BAYC
    .add_address(
        "0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D",
        &Chain::Mainnet,
        17773490,
    );

    // Setup Doodles' contract
    let doodles_contract =  Contract::new("Doodles")
    .add_event("event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)", TransferEventHandler)
    .add_address(
      "0x8a90CAb2b38dba80c64b7734e58Ee1dB38B8992e",
      &Chain::Mainnet,
      17769635,
    );


    // Setup indexing config
    let config = Config::new(
      // Choose your database and provider its corresponding url
      PostgresRepo::new("postgres://postgres:postgres@localhost/example-db"),
      HashMap::from([(
          Chain::Mainnet,
          "https://eth-mainnet.g.alchemy.com/v2/some-secret"
      )]),
    )
    // add BAYC's and Doodles' contracts
    .add_contract(bayc_contract)
    .add_contract(doodles_contract);


    // Start Indexing Process
    Chaindexing::index_states(&config).await.unwrap();
}
```
