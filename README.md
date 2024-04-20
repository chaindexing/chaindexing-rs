# Chaindexing

[<img alt="github" src="https://img.shields.io/badge/Github-jurshsmith%2Fchaindexing-blue?logo=github" height="20">](https://github.com/jurshsmith/chaindexing-rs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/chaindexing.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/chaindexing)
[<img alt="diesel-streamer build" src="https://img.shields.io/github/actions/workflow/status/jurshsmith/chaindexing-rs/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/jurshsmith/chaindexing-rs/actions?query=branch%3Amain)

Index any EVM chain and query in SQL

[Getting Started](#getting-started) | [Examples](https://github.com/chaindexing/chaindexing-examples/tree/main/rust) | [Design Goals & Features](#design-goals--features) | [RoadMap](#roadmap) | [Contributing](#contributing)

## Getting Started

📊 Here is what indexing and tracking owers of your favorite NFTs looks like:

```rust
use chaindexing::states::{ContractState, Filters, Updates};
use chaindexing::{EventContext, EventHandler, SideEffectContext, SideEffectHandler};

use crate::states::Nft;

pub struct TransferHandler;

#[chaindexing::augmenting_std::async_trait::async_trait]
impl EventHandler for TransferHandler {
    fn abi(&self) -> &'static str {
        "event Transfer(address indexed from, address indexed to, uint256 indexed tokenId)"
    }
    async fn handle_event<'a, 'b>(&self, context: EventContext<'a, 'b>) {
        let event_params = context.get_event_params();

        let _from = event_params.get_address_string("from");
        let to = event_params.get_address_string("to");
        let token_id = event_params.get_u32("tokenId");

        if let Some(existing_nft) =
            Nft::read_one(&Filters::new("token_id", token_id), &context).await
        {
          let updates = Updates::new("owner_address", &to);
          existing_nft.update(&updates, &context).await;
        } else {
            let new_nft = Nft {
                token_id,
                owner_address: to,
            };

            new_nft.create(&context).await;
        }
    }
}
```

A quick and effective way to get started is by exploring the comprehensive examples provided here: [https://github.com/chaindexing/chaindexing-examples/tree/main/rust](https://github.com/chaindexing/chaindexing-examples/tree/main/rust).

## Design Goals & Features

- 💸&nbsp;Free forever<br/>
- ⚡&nbsp;Real-time use-cases<br/>
- 🌐&nbsp;Multi-chain<br/>
- 🧂&nbsp;Granular, 🧩 Modular & 📈 Scalable<br/>
- 🌍&nbsp;Environment-agnostic to allow inspecting 🔍 & replicating indexes anywhere!<br/>
- 🔓&nbsp;ORM-agnostic, use any ORM to access indexed data<br/>
- 📤&nbsp;Easy export to any data lake: S3, Snowflake, etc.<br/>
- 🚫&nbsp;No complex YAML/JSON/CLI config<br/>
- 💪&nbsp;Index contracts discovered at runtime<br/>
- ✨&nbsp;Handles re-org with no UX impact<br/>
- 🔥&nbsp;Side effect handling for notifications & bridging use cases<br/>
- 💸&nbsp;Optimize RPC cost by indexing when certain activities happen in your DApp<br/>
- 💎&nbsp;Language-agnostic, so no macros!<br/>

## RoadMap

- ⬜&nbsp;Expose `is_at_block_tail` flag to improve op heuristics for applications<br/>
- ⬜&nbsp;Support SQLite Database (Currently supports only Postgres)<br/>
- ⬜&nbsp;Support indexing raw transactions & call traces.<br/>
- ⬜&nbsp;Improved error handling/messages/reporting (Please feel free to open an issue when an opaque runtime error is encountered)<br/>
- ⬜&nbsp;SSL Support<br/>
- ⬜&nbsp;Minimal UI for inspecting events and indexed states<br/>

## Contributing

All contributions are welcome. Before working on a PR, please consider opening an issue detailing the feature/bug. Equally, when submitting a PR, please ensure that all checks pass to facilitate a smooth review process.
