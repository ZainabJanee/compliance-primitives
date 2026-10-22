# CI/CD Documentation

This document describes the CI workflows and jobs that run against pull requests and main branch pushes.

## Regular CI Jobs (.github/workflows/ci.yml)

These jobs run on every PR and push to main:

### cargo test
Runs `cargo test --workspace --verbose` and doc tests. Verifies all unit tests pass.

### cargo clippy
Runs `cargo clippy --workspace --all-targets` with warnings as errors. Enforces code quality and style.

### budget regression checks
Runs `cargo test --workspace budget_regression`. Catches unintended instruction budget bloat in contracts.

### wasm32v1-none build
Builds `cargo build --workspace --target wasm32v1-none --release`. Ensures all contracts compile to valid wasm.

### wasm size budget
Runs `scripts/check-wasm-size.sh` (locally: `make check-wasm-size`). Builds every contract listed in `wasm-size-budgets.toml` — including `allowlist-token`, `denylist-gate`, and `jurisdiction-flag` — to `wasm32v1-none` and fails if a binary exceeds its recorded baseline by more than 10%. On failure it names the contract and reports how many bytes it is over its limit and its baseline. See the header of `wasm-size-budgets.toml` for the budget rationale and how to update a baseline.

### cargo deny check
Runs `cargo-deny` to check for:
- Dependency security advisories (via RustSec)
- License compliance issues
- Multiple versions of the same crate
- Unknown registries or git sources

### cargo semver-checks
Runs `cargo-semver-checks check-release` against publishable crates to catch accidental breaking API changes before merge.

**Scope:** Currently checks `compliance-pausable` (the primary shared/publishable crate). Add `--package` flags when additional crates become public.

## Scheduled Jobs (.github/workflows/scheduled-deny.yml)

### Scheduled cargo-deny check (weekly)
Runs every Sunday at 00:00 UTC to catch newly disclosed advisories against already-merged dependencies.

**Failure handling:** If a new advisory is detected, the job:
1. Searches for an existing open security tracking issue
2. Comments on the existing issue if found, or creates a new one
3. Includes a link to the failed workflow run for investigation

This ensures that a new advisory doesn't go unnoticed between regular dependency updates.

## Local Equivalents

To run the same checks locally before committing:

```sh
make test        # Runs cargo test (mirrors the CI test job)
make lint        # Runs clippy and formatting checks (mirrors CI lint jobs)
make build       # Builds wasm for all contracts (mirrors wasm build job)
cargo deny check # Runs dependency checks locally
```

## Adding New CI Jobs

When adding a new CI job:

1. Add the job to `.github/workflows/ci.yml` (for regular checks) or create a new workflow file (for scheduled/specialized jobs)
2. Document the job's purpose and scope here
3. Update this document with the job's description, what it checks, and when it runs

## References

- [GitHub Actions documentation](https://docs.github.com/en/actions)
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/)
- [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)
