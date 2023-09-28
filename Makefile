db.start:
	docker-compose up

db.stop: 
	docker-compose down

db.drop:
	rm -rf ./postgres-data

db.reset:
	make db.stop && make db.drop && make db.start

tests.setup:
	cargo run -p chaindexing-tests

tests:
	cargo test -- --nocapture

tests.after_setup:
	make tests.setup && make tests

tests.with_backtrace:
	RUST_BACKTRACE=1 make tests

doc:
	cargo doc --open

publish.dirty:
	cargo publish -p chaindexing --allow-dirty