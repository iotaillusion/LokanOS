.PHONY: help fmt lint test build e2e package sbom oas

BUILD_SHA ?= $(shell git rev-parse --short HEAD)
BUILD_TIME ?= $(shell date -u +"%Y-%m-%dT%H:%M:%SZ")

CARGO_ENV = BUILD_SHA=$(BUILD_SHA) BUILD_TIME=$(BUILD_TIME)

help:
	@echo "Available targets: fmt lint test build e2e package sbom oas"

fmt:
	$(CARGO_ENV) cargo fmt --all

lint:
	$(CARGO_ENV) cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	$(CARGO_ENV) cargo test --workspace --all-targets

build:
	$(CARGO_ENV) cargo build --workspace --all-targets

e2e:
	@echo "e2e tests are not implemented yet"

package:
	@echo "Packaging pipeline is not implemented yet"

sbom:
	@echo "SBOM generation is not implemented yet"

oas:
	$(CARGO_ENV) cargo run --quiet --manifest-path tools/Cargo.toml --bin oas-bundle
