up:
	docker-compose up

down: 
	docker-compose down

tests.setup:
	cargo run -p chaindexing-tests

tests:
	cargo test -p chaindexing-tests -- --nocapture

tests.with_backtrace:
	RUST_BACKTRACE=1 cargo test -p chaindexing-tests -- --nocapture

doc:
	cargo doc --open