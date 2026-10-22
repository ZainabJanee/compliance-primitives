# Full RWA token composition: feasibility note

> **Not a deployable example.** This note records why issue #437 is not
> implemented by this change; its acceptance criteria remain open.

The requested nine-contract deployment does not match the current package
boundaries:

- [`pausable`](../../contracts/pausable/src/lib.rs) is a Rust helper library,
  not a Soroban contract. It exports functions that a contract can call with
  its own `Env`; it has no contract entry point or independently deployable
  contract ID.
- [`audit-log`](../../contracts/audit-log/src/lib.rs) exposes `record`, but
  other contracts must explicitly integrate a client call for events to be
  written. Deploying the log alone does not make token transfers appear in it.
- [`multisig-admin`](../../contracts/multisig-admin/src/lib.rs) is a Soroban
  custom account. Admin operations require the configured signer addresses to
  authorize each invocation, so a realistic deployment example must document
  and test that authorization path rather than treating it as a regular admin
  key.
- `compliance-aggregator` and `policy-engine` have overlapping denylist,
  jurisdiction, and circuit-breaker checks. A full example needs to define
  whether both are intentionally called on each transfer and test the added
  cross-contract call cost.

Before adding a deployment-ready example, clarify whether the requirement is
to include `compliance-pausable` as an embedded library rather than as a
separately deployed contract, then define and test the multisig authorization
and audit-log integration flows. This change intentionally does not claim to
resolve #437.