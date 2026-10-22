# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **audit-log**: Admin-gated `upgrade(admin, new_wasm_hash)` entrypoint with an
  `UpgradePerformed` event, module-level upgrade documentation, and an
  `UPGRADE.md` describing the upgrade procedure and storage rules (#187)
- **compliance-aggregator**: Admin-gated `upgrade(admin, new_wasm_hash)`
  entrypoint with an `UpgradePerformed` event, a migration test that
  deploys, writes state, upgrades, and verifies state is intact and callable,
  and module-level upgrade documentation (#195)
- **compliance-aggregator**: `check_address` budget-regression benchmark
  with a `[compliance-aggregator.check_address]` entry in
  `budget-baselines.toml` (#196)
- **multisig-admin**: Expanded module-level docs covering purpose, callers,
  and composition with the other contracts in the repo (#197)
- **multisig-admin**: Instance TTL is extended on every write path
  (`INSTANCE_TTL_THRESHOLD` / `INSTANCE_TTL_EXTEND_TO`), with tests that
  advance the ledger past the original TTL (#198)
- **multisig-admin**: `upgrade(new_wasm_hash)` entrypoint, gated by the M-of-N
  threshold, with a migration test confirming state survives the upgrade
- **multisig-admin**: `__check_auth` resource-fee benchmark and
  `[multisig-admin.__check_auth]` baseline in `budget-baselines.toml`
- **policy-engine**: Instance-storage TTL extension on every write path
  (threshold ~7 days, extend-to ~90 days)

### Changed

- **policy-engine**: Expanded module-level docs covering purpose, callers,
  composition with the other contracts, and the TTL policy

## [0.1.0] — 2026-08-27

### Added

- **multisig-admin**: Proposal workflow with ledger-sequence-based expiry
  - `propose()` — Create a new governance proposal with an expiry ledger
  - `approve()` — Approve a proposal (restricted to registered signers)
  - `execute()` — Execute a proposal once threshold is met
  - `get_proposal()` — Query proposal details and current approvals
  - `ExpiredProposal` error variant — Rejects approval/execution after expiry

- **multisig-audit-trail**: New example demonstrating governance audit trail
  - Shows how to integrate `multisig-admin` proposals with `audit-log`
  - Pattern for recording proposal creation, approvals, execution, and expiry
  - Test suite covering successful proposal flow, expiry handling, and approval history

- **CI**: Changelog version sync check
  - `scripts/check-changelog.sh` enforces CHANGELOG.md updates alongside version bumps
  - New GitHub Actions job `changelog-version-sync` validates in CI

- **Documentation**: QUICKSTART.md
  - Step-by-step guide from clone → build → test → example → testnet → web playground
  - Linked prominently from README.md
  - Includes troubleshooting and key file reference

### Changed

- README.md: Added prominent link to QUICKSTART.md for new contributors
- CONTRIBUTING.md: Documented CHANGELOG.md requirement for version bumps
