# Pre-Mainnet Deployment Checklist

<!-- Resolves #461 -->

> **How to use this checklist:** Work through every section before promoting
> any deployment of these contracts to Stellar mainnet. Check off each item
> as completed. If an item cannot be completed, document why and obtain
> explicit sign-off from the security lead before proceeding.

---

## 1. Build Integrity

- [ ] All contracts build cleanly with `cargo build-wasm` (zero errors,
  zero warnings with `-D warnings`).
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] WASM sizes for every deployable contract are within the budgets in
  `wasm-size-budgets.toml` (verify with `scripts/check-wasm-size.sh`).
- [ ] Reproducible-build check passes: `scripts/verify-reproducible-builds.sh`
  produces identical hashes on two independent builds.
- [ ] The Rust toolchain version in `rust-toolchain.toml` matches what CI uses.

---

## 2. Full-Workspace Test Pass

> **Why this section exists:** Previous merges have introduced defects that
> passed per-contract CI (each contract tested in isolation) but were only
> visible when contracts were composed together. _Do not skip this section
> even if all per-contract tests pass._

- [ ] `cargo test --workspace` passes with zero test failures.
- [ ] Every example under `examples/` builds: `cargo build --workspace`.
- [ ] Integration tests that exercise multiple contracts together have been
  run and pass (e.g. `examples/rwa-token`, `examples/compliance-integration-flow`,
  `examples/circuit-breaker-policy-engine`).
- [ ] Fuzz targets (if any) have been run for at least one hour with no
  panics: `fuzz/README.md` for instructions.

---

## 3. Composition Feature Tests

> **Why this section exists:** Features that work correctly in isolation
> have previously failed when composed. Each item below covers a known
> composition path that must be tested end-to-end.

- [ ] `denylist-gate` with an `audit-log` wired in: verify that a denylist
  mutation records an entry in the audit log and that a panicking audit-log
  address correctly aborts the mutation (expected behaviour, not a bug).
- [ ] `policy-engine` with at least one `CheckKind::Denylist` and one
  `CheckKind::Jurisdiction` check registered: verify `evaluate` returns
  the correct pass/fail for all four combinations (both pass, first fails,
  second fails, both fail) under both `CombineOp::All` and `CombineOp::Any`.
- [ ] `compliance-aggregator` with both `denylist-gate` and
  `jurisdiction-flag` registered: verify `check_all` returns per-check
  breakdowns consistent with the individual contract calls.
- [ ] `circuit-breaker` wired as the emergency stop in both `policy-engine`
  and `compliance-aggregator`: verify that freezing the circuit-breaker
  causes both to return failure without calling the underlying checks.
- [ ] `multisig-admin` as the admin of at least one primitive: verify that
  a mutation requiring admin auth is rejected unless the multisig threshold
  is met.
- [ ] Jurisdiction expiry (`set_jurisdiction_until`): verify that an address
  with an expired jurisdiction code is treated as non-compliant by both
  `jurisdiction-flag.is_permitted_jurisdiction` and `policy-engine.evaluate`.

---

## 4. Authorization & Access-Control Audit

- [ ] Every admin-gated mutating function across all nine contracts has an
  explicit negative-auth test that confirms it rejects a non-admin caller
  (see issue #462 and the audit in `contracts/*/src/test.rs`).
- [ ] Compliance-officer delegation has been reviewed against
  `docs/compliance-officer-delegation-threat-model.md`: no officer can
  escalate to admin, and revocation works without officer co-operation.
- [ ] The stored admin/issuer address for every deployed contract has been
  verified to be the intended multisig or cold-storage address (not a hot
  wallet or test key).
- [ ] Standing compliance-officer delegations have been reviewed: if a
  delegation was assigned for a one-off task, it has been revoked.
- [ ] The cross-contract call site audit in
  `docs/cross-contract-trust-assumptions.md` has been reviewed: every
  callee address (audit-log, denylist-gate, jurisdiction-flag,
  circuit-breaker, allowlist-token) is the intended deployed contract.

---

## 5. Known Gaps / Issues Review

> Review the open issue tracker for any issues labelled `known-gap` or
> `security` before deploying. The items below were open at the time this
> checklist was written.

- [ ] All open `security`-labelled issues have been reviewed; any that are
  `complexity: high` or have a deployment dependency have a documented
  risk-acceptance decision.
- [ ] `compliance-officer delegation` events gap (identified in #459): the
  absence of on-chain events for officer set/revoke is accepted as low-risk
  for this deployment, OR dedicated events have been added.
- [ ] Batch-size budget: verify that the batch sizes used in
  `denylist-gate.remove_multiple_from_denylist` (max 45) and
  `compliance-aggregator.batch_check` (max 45) do not exceed the
  resource budget on the target network configuration.
- [ ] Jurisdiction expiry: confirm that the deployed `jurisdiction-flag`
  instance enforces `valid_until` correctly and that any addresses with
  expired codes have been explicitly re-flagged or removed.
- [ ] Delegated admin key (allowlist-token): if `set_delegated_admin_key`
  has been called, verify the key is stored securely and the relayer has
  nonce tracking in place.

---

## 6. Deployment Configuration

- [ ] `initialize` has been called on every deployed contract with the
  correct admin/issuer address.
- [ ] No contract has been left in a state where `initialize` has not been
  called (calling any mutating function before `initialize` returns
  `NotInitialized`).
- [ ] Contract IDs for all nine deployments have been recorded and are
  accessible to the operations team.
- [ ] Cross-contract wiring is correct:
  - `denylist-gate`: `set_audit_log` called with the correct audit-log
    address (if using the audit-log integration).
  - `policy-engine`: each `add_check` call references the correct deployed
    contract address for its check type.
  - `compliance-aggregator`: `set_denylist_gate`, `set_jurisdiction_flag`,
    and (if used) `set_circuit_breaker` reference the correct deployed
    contract addresses.
- [ ] Pause state: all contracts are in the unpaused state before go-live
  (unless intentionally paused for a delayed launch).

---

## 7. Testnet Smoke Test

- [ ] All contracts have been deployed to Stellar testnet and the full
  deployment walkthrough in `docs/full-stack-testnet-walkthrough.md` has
  been executed successfully.
- [ ] A representative transfer has been evaluated end-to-end:
  `allowlist-token.transfer` (or a consumer token calling `policy-engine`)
  successfully clears all compliance gates.
- [ ] A representative blocked transfer has been verified: a non-allowlisted
  or denylisted address is correctly rejected.
- [ ] The `audit-log` contains the expected entries from the smoke-test
  denylist mutations.
- [ ] The `circuit-breaker` freeze/unfreeze cycle has been tested on testnet.

---

## 8. Security Review Sign-off

- [ ] This checklist has been completed in full by the deploying team.
- [ ] An independent reviewer (not the deployer) has spot-checked at least
  sections 3, 4, and 5.
- [ ] All items marked with a documented exception have been approved by the
  security lead in writing.
- [ ] The deployment transaction (including `initialize` calls) has been
  reviewed and signed by the multisig threshold (if `multisig-admin` is
  used as the admin).

---

*Document authored as part of issue #461. For the cross-contract trust audit
see `docs/cross-contract-trust-assumptions.md`. For vulnerability reporting
see `SECURITY.md`.*
