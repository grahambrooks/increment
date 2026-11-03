# Convenience targets. `make check` is the gate: it runs exactly what the CI
# `check` job runs (.github/workflows/build.yml), plus the pure-Rust check, and
# must pass before every commit. `make hooks` runs it before every push.
#
# Releases come from release.mk (release-kit): `make release` tags origin/main
# with the next CalVer (today's UTC date, vYYYY.M.D, last component bumped if
# that tag exists) and pushes only the tag. The release workflow stamps the
# version into Cargo.toml for the build, then its release PR bumps Cargo.toml in
# the repo alongside the Homebrew formula. Nothing is bumped or committed here,
# so there is nothing to race the bot.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

.PHONY: help
help: ## List targets
	@grep -hE '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{ printf "  %-16s %s\n", $$1, $$2 }'

.PHONY: check
check: pure-rust fmt-check lint test ## The gate: the CI check job (fmt, clippy, tests) and no-C-deps

# The pure-Rust constraint, as a check rather than a comment. syntect and
# two-face both default to the `onig` C library, and cargo unifies features
# across the graph — so one of them reverting to defaults quietly reintroduces
# a C toolchain dependency that nothing else here would catch.
.PHONY: pure-rust
pure-rust: ## Fail if the onig C library is in the dependency tree
	@if cargo tree -e normal,build 2>/dev/null | grep -qiE 'onig|onig_sys'; then \
		echo 'pure-rust: the onig C library is back in the dependency tree'; \
		cargo tree -i onig; \
		exit 1; \
	fi

.PHONY: build
build: ## Debug build
	cargo build

.PHONY: test
test: ## Tests, as CI runs them
	cargo test --workspace --all-features --locked

.PHONY: lint
lint: ## Clippy with warnings denied, as CI runs it
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

.PHONY: fmt
fmt: ## Format in place
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Fail if the code is not formatted
	cargo fmt --all --check

.PHONY: hooks
hooks: ## Install the prek hooks (pre-commit and pre-push)
	prek install

include release.mk
