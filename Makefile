# Convenience targets. `make check` is the pre-commit gate and must pass before
# every commit.
#
# CalVer is YYYY.M.MICRO, where MICRO counts the releases already cut this
# month. The month has no leading zero: 2026.08.1 is not valid SemVer, and
# cargo requires SemVer.
#
# The tag is the version (release-kit v2). `make release` only tags origin/main
# and pushes the tag; the release workflow stamps the version into Cargo.toml
# for the build, then its release PR bumps Cargo.toml in the repo alongside the
# Homebrew formula. Nothing is bumped or committed here, so there is nothing to
# race the bot.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

YEAR    := $(shell date -u +%Y)
MONTH   := $(shell date -u +%-m)
# One past the highest MICRO already tagged this month, not the *number* of
# tags. Counting assumes no tag is ever deleted or skipped: delete one and the
# next release silently reuses a version that is already published.
MICRO    = $(shell last=$$(git tag -l 'v$(YEAR).$(MONTH).*' \
             | sed -n 's/^v[0-9][0-9]*\.[0-9][0-9]*\.\([0-9][0-9]*\)$$/\1/p' \
             | sort -n | tail -1); echo $$(( $${last:--1} + 1 )))
VERSION  = $(YEAR).$(MONTH).$(MICRO)
TAG      = v$(VERSION)

.PHONY: help
help:
	@echo 'make check        fmt, clippy, tests, no-C-deps — the pre-commit gate'
	@echo 'make build        debug build'
	@echo 'make test         tests only'
	@echo 'make fmt          format in place'
	@echo 'make hooks        install the prek hooks'
	@echo 'make version      print the next version'
	@echo 'make release-dry  show what a release would do, without doing it'
	@echo 'make release      tag origin/main as $(TAG) and push the tag'

.PHONY: check
check: pure-rust
	cargo fmt --all --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

# The pure-Rust constraint, as a check rather than a comment. syntect and
# two-face both default to the `onig` C library, and cargo unifies features
# across the graph — so one of them reverting to defaults quietly reintroduces
# a C toolchain dependency that nothing else here would catch.
.PHONY: pure-rust
pure-rust:
	@if cargo tree -e normal,build 2>/dev/null | grep -qiE 'onig|onig_sys'; then \
		echo 'pure-rust: the onig C library is back in the dependency tree'; \
		cargo tree -i onig; \
		exit 1; \
	fi

.PHONY: build
build:
	cargo build

.PHONY: test
test:
	cargo test --all-features

.PHONY: fmt
fmt:
	cargo fmt --all

.PHONY: hooks
hooks:
	prek install --hook-type pre-commit --hook-type pre-push

.PHONY: version
version:
	@echo '$(VERSION)'

.PHONY: release-dry
release-dry: release-fetch
	@echo 'would tag origin/main ($(shell git rev-parse --short origin/main)) as $(TAG) and push the tag'
	@echo 'the tag push runs the release workflow: it builds the binaries, publishes'
	@echo 'a GitHub Release, and merges a PR that bumps Cargo.toml and the formula'

# Tags come from origin so MICRO is computed from what is published, not from
# whatever this clone happens to have. A prerequisite rather than a recipe line,
# so the fetch lands before $(TAG) is expanded.
.PHONY: release-fetch
release-fetch:
	@git fetch --quiet origin main --tags

.PHONY: release
release: release-fetch
	@! git rev-parse -q --verify 'refs/tags/$(TAG)' >/dev/null \
		|| { echo 'release: tag $(TAG) already exists'; exit 1; }
	@echo '--> tagging origin/main as $(TAG)'
	git tag -a '$(TAG)' -m 'increment $(VERSION)' origin/main
	git push --quiet origin '$(TAG)'
	@echo 'pushed $(TAG); the release workflow takes it from here'
