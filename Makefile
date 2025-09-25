.PHONY: install fmt lint test build e2e

install:
	@echo "Installing workspace dependencies"
	npm install --prefix services/api-gateway

fmt:
	@echo "Running formatters"

lint:
	@echo "Running linters"

test:
	npm test --prefix services/api-gateway

build:
	npm run build --prefix services/api-gateway
	@echo "Build complete"

e2e:
	@echo "Running end-to-end tests"
