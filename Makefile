up:
	docker-compose up

tests.setup:
	cargo run -p chaindexing-tests

test:
	cargo test -p chaindexing-tests --lib

doc:
	cargo doc --open