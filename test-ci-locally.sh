#!/bin/bash

# Simulate CI environment
export TEST_DATABASE_URL="postgres://postgres:postgres@localhost:5432/chaindexing_tests"

echo "🚀 Simulating GitHub CI workflow locally..."
echo ""

echo "📦 Step 1: Database Setup"
cargo run -p chaindexing-tests
if [ $? -ne 0 ]; then
    echo "❌ Database setup failed"
    exit 1
fi
echo "✅ Database setup completed"
echo ""

echo "🧪 Step 2: Running Tests (with parallel execution)"
SETUP_TEST_DB=1 cargo test
if [ $? -ne 0 ]; then
    echo "❌ Tests failed"
    exit 1
fi
echo "✅ Tests passed"
echo ""

echo "📎 Step 3: Clippy Check (strict mode)"
cargo clippy --tests -- -Dclippy::all
if [ $? -ne 0 ]; then
    echo "❌ Clippy check failed"
    exit 1
fi
echo "✅ Clippy check passed"
echo ""

echo "📝 Step 4: Format Check"
rustfmt --check --edition 2021 $(git ls-files '*.rs')
if [ $? -ne 0 ]; then
    echo "❌ Format check failed"
    exit 1
fi
echo "✅ Format check passed"
echo ""

echo "🎉 All CI checks would pass! GitHub CI should be green."
echo "🚀 Tests now run in parallel for faster execution!" 