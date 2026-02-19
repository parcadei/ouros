# CPython Conformance Differential Test Harness
# Runs Python snippets through both CPython and ouros, comparing output.

OUROS_ROOT := /tmp/ouro-fresh
OUROS_BIN  := $(OUROS_ROOT)/target/release/ouros

.PHONY: test build test-verbose test-json test-junit clean help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  %-15s %s\n", $$1, $$2}'

build: ## Build ouros CLI (release mode)
	cd $(OUROS_ROOT) && cargo build -p ouros-cli --release

test: build ## Run conformance tests
	python3 harness.py --ouros-bin $(OUROS_BIN)

test-verbose: build ## Run conformance tests with verbose output
	python3 harness.py --ouros-bin $(OUROS_BIN) --verbose

test-json: build ## Run tests and output JSON to results.json
	python3 harness.py --ouros-bin $(OUROS_BIN) --json > results.json

test-junit: build ## Run tests and output JUnit XML to results.xml
	python3 harness.py --ouros-bin $(OUROS_BIN) --junit results.xml

clean: ## Remove generated result files
	rm -f results.json results.xml
