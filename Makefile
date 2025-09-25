.PHONY: help fmt lint test build e2e package sbom

help:
@echo "Available targets: fmt lint test build e2e package sbom"

fmt:
cargo fmt --all

lint:
cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
cargo test --workspace --all-targets

build:
cargo build --workspace --all-targets

e2e:
@echo "e2e tests are not implemented yet"

package:
@echo "Packaging pipeline is not implemented yet"

sbom:
@echo "SBOM generation is not implemented yet"
