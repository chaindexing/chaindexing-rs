up:
	docker-compose up

tests:
	cargo test -p chaindexing-tests --lib

doc:
	cargo doc --open