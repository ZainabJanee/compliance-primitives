.PHONY: test doctest clippy fmt fmt-check lint build check-wasm-size check-accessibility check-all check-contracterror-discriminants

# Mirrors the "cargo test" CI job (.github/workflows/ci.yml).
test:
	cargo test --workspace --verbose

# Mirrors the "Run doc tests" CI step.
doctest:
	cargo test --workspace --doc --verbose

# Mirrors the "cargo clippy" CI job.
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

# Full local lint pass: clippy (as run in CI) plus a formatting check.
lint: clippy fmt-check check-contracterror-discriminants

check-contracterror-discriminants:
	python3 scripts/check-contracterror-discriminants.py

# Mirrors the "wasm32v1-none build" CI job.
build:
	cargo build --workspace --target wasm32v1-none --release

# Mirrors the "wasm size budget" CI job.
check-wasm-size:
	bash scripts/check-wasm-size.sh

# Mirrors the "accessibility audit (web/)" CI job. Run `npm install` once
# first (downloads Puppeteer's bundled Chromium).
check-accessibility:
	node scripts/check-accessibility.mjs

# Run cargo check -p over every workspace member, continuing past failures,
# then print a pass/fail summary table.  Useful for workspace-wide triage when
# a single `cargo check --workspace` would stop at the first broken crate.
#
#   make check-all            # cargo check for each crate
#   make check-all ARGS=--test    # also run cargo test per crate
#   make check-all ARGS=--clippy  # also run clippy per crate
check-all:
	bash scripts/check-all.sh $(ARGS)