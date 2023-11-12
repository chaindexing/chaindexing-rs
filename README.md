# Chaindexing

[<img alt="github" src="https://img.shields.io/badge/Github-jurshsmith%2Fchaindexing-blue?logo=github" height="20">](https://github.com/jurshsmith/chaindexing-rs)
[<img alt="crates.io" src="https://img.shields.io/crates/v/chaindexing.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/chaindexing)
[<img alt="diesel-streamer build" src="https://img.shields.io/github/actions/workflow/status/jurshsmith/chaindexing-rs/ci.yml?branch=main&style=for-the-badge" height="20">](https://github.com/jurshsmith/chaindexing-rs/actions?query=branch%3Amain)

An EVM indexing engine that lets you query chain data with SQL.

### Mini Comparison with TheGraph

It is a great alternative to theGraph [https://thegraph.com/](https://thegraph.com/) if you:

- have a server + relational database setup
- are NOT indexing thousands of contracts
- don't want to deal with an additional external system
- have written your DApp in RUST (Other Languages soon to come!)

### Examples

[https://github.com/chaindexing/chaindexing-examples/tree/main/rust](https://github.com/chaindexing/chaindexing-examples/tree/main/rust) contains examples that can be quickly tested and replicated. While the actual documentation is being worked on, feel free to use them as templates and open issues if they don't work correctly.
