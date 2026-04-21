.PHONY: all format lint check audit test build

all: format lint check test build

format:
	@echo "Running cargo fmt..."
	cargo fmt

lint:
	@echo "Running cargo clippy..."
	cargo clippy -- -D warnings

check:
	@echo "Running cargo check..."
	cargo check

audit:
	@echo "Running cargo audit..."
	# Requires cargo-audit to be installed
	cargo audit

test:
	@echo "Running tests..."
	cargo test

build:
	@echo "Building release binary..."
	cargo build --release