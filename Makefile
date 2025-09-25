.PHONY: help fmt lint test

help:
@echo "Available targets: fmt lint test"

fmt:
cargo fmt --all

lint:
cargo clippy --all-targets --all-features -- -D warnings

test:
cargo test --all
