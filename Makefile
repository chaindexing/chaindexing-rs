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

tests.without.capture: 
	make tests.setup && cargo test -- --nocapture

tests.with.name:
	cargo test -p chaindexing-tests -- $(name)

tests.with.name.and.backtrace:
	RUST_BACKTRACE=1 cargo test -p chaindexing-tests -- $(name)

tests.with.backtrace:
	RUST_BACKTRACE=1 make tests

doc:
	cargo doc --open

publish:
	cargo publish -p chaindexing

publish.dirty:
	cargo publish -p chaindexing --allow-dirty