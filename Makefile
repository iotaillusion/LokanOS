.PHONY: help fmt lint test build e2e package sbom oas sdks sdk-c

BUILD_SHA ?= $(shell git rev-parse --short HEAD)
BUILD_TIME ?= $(shell date -u -Iseconds | sed 's/+00:00/Z/')

CARGO_ENV = BUILD_SHA=$(BUILD_SHA) BUILD_TIME=$(BUILD_TIME)
FAST ?= 0

help:
	@echo "Available targets: fmt lint test build e2e package sbom oas sdks sdk-c"

fmt:
	$(CARGO_ENV) cargo fmt --all

lint:
	@if [ "$(FAST)" = "1" ]; then \
	        echo "FAST=1 skipping $@"; \
	else \
	        $(CARGO_ENV) cargo clippy --workspace --all-targets --all-features -- -D warnings; \
	fi

test:
	@if [ "$(FAST)" = "1" ]; then \
	        echo "FAST=1 skipping $@"; \
	else \
	        $(CARGO_ENV) cargo test --workspace --all-targets; \
	fi

build:
	@if [ "$(FAST)" = "1" ]; then \
	        echo "FAST=1 skipping $@"; \
	else \
	        $(CARGO_ENV) cargo build --workspace --all-targets; \
	fi

e2e:
	@echo "e2e tests are not implemented yet"

package:
	@echo "Packaging pipeline is not implemented yet"

sbom:
	@echo "SBOM generation is not implemented yet"

oas:
	@if [ "$(FAST)" = "1" ]; then \
		echo "FAST=1 skipping $@"; \
	else \
		$(CARGO_ENV) cargo run --quiet --manifest-path tools/Cargo.toml --bin oas-bundle; \
	fi

sdks: oas
	@if [ "$(FAST)" = "1" ]; then \
	        echo "FAST=1 skipping $@"; \
	else \
	        node tools/oas2ts.ts; \
	fi

sdk-c:
	@if [ "$(FAST)" = "1" ]; then \
	        echo "FAST=1 skipping $@"; \
	else \
	        cmake -S sdks/c -B sdks/c/build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$(CURDIR)/sdks/c/dist; \
	        cmake --build sdks/c/build --config Release; \
	        cmake --install sdks/c/build --config Release; \
	fi
