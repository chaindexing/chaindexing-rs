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
	make tests.setup && cargo test -- --nocapture

tests.with_backtrace:
	RUST_BACKTRACE=1 make tests

doc:
	cargo doc --open

publish:
	cargo publish -p chaindexing

publish.dirty:
	make publish --allow-dirty