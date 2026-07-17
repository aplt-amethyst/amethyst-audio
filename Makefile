# amethyst-audio Makefile
# CPS v0.1.0-4b compliant build system

CARGO := cargo
BIN := target/debug/amethyst-audio

.PHONY: help build build.debug build.release run test examples clean format install uninstall check deb

help: ## Print all available build targets
	@printf "\033[1;36mamethyst-audio\033[0m — HLS Stream Server\n"
	@printf "\033[1;33mAvailable targets:\033[0m\n"
	@grep -E '^[a-zA-Z_.-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[1;32m%-20s\033[0m %s\n", $$1, $$2}'

build: build.debug ## Compile in debug mode

build.debug: ## Compile in debug mode
	$(CARGO) build

build.release: ## Compile in release mode (O3 + LTO)
	$(CARGO) build --release

run: ## Build and run in debug mode
	$(CARGO) run

test: ## Run unit and integration tests
	$(CARGO) test

examples: ## Build all examples
	$(CARGO) build --examples

clean: ## Clean build artifacts
	$(CARGO) clean

format: ## Format source code (cargo fmt)
	$(CARGO) fmt

install: build.release ## Install binary to system path
	cp target/release/amethyst-audio /usr/local/bin/amethyst-audio

uninstall: ## Uninstall binary from system path
	rm -f /usr/local/bin/amethyst-audio

check: ## Run clippy lints (fast check, no compilation)
	$(CARGO) clippy -- -D warnings

deb: build.release ## Build Debian package (.deb)
	@if [ -f target/release/amethyst-audio.exe ]; then \
		cp target/release/amethyst-audio.exe target/release/amethyst-audio.bin; \
	fi
	$(CARGO) deb
