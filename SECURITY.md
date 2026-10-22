# Security Policy

## Scope

These contracts are compliance-critical primitives meant for real issuers.
In scope for a security report:

- Anything that lets a transfer go through despite the sender or recipient
  being denylisted, not allowlisted, or in a disallowed jurisdiction.
- Anything that lets a non-admin/non-issuer bypass `require_auth()` and
  mutate allowlist, denylist, or jurisdiction state.
- Anything that causes incorrect or missing compliance events, in a way
  that would hide a compliance-relevant state change from off-chain
  monitoring.

Out of scope: issues in the example contracts under `/examples` that don't
affect the primitives themselves, and purely cosmetic/documentation issues
(please file those as a regular GitHub issue instead).

## Contract upgradeability

The following contracts expose an upgrade path:

- `allowlist-token` and `denylist-gate`: the admin must propose an upgrade,
  then wait until its configured ledger delay has elapsed before committing
  it. A pending upgrade can be cancelled by the admin during the delay.
- `jurisdiction-flag`: the issuer can replace the contract Wasm immediately
  with `upgrade()`; no delay or cancellation window is built into this path.

Soroban keeps the contract ID and its instance and persistent storage when
the Wasm is replaced. This includes the admin or issuer and compliance state
stored under compatible keys. An upgrade does not migrate, reinterpret, or
repair storage: the new code must remain compatible with existing key and
value types, or provide and invoke an explicit migration. In
`allowlist-token` and `denylist-gate`, the pending-upgrade record is removed
when an upgrade is committed; it is not carried forward as a pending action.
The previous Wasm is not retained as an automatic rollback target, and a
failed migration or incompatible schema requires a separately deployed
corrective upgrade.

Other contracts in this repository do not currently expose an upgrade
entrypoint. Review a contract's authorization and storage compatibility
before relying on an upgrade path for production deployments.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report privately via [GitHub Security Advisories](https://github.com/stellar-compliance-kit/compliance-primitives/security/advisories/new)
for this repository. Include the affected contract(s), a description of
the issue, and steps to reproduce if possible.

## What to expect

We aim to acknowledge new reports within a few business days and to keep
you updated as we investigate and work on a fix. Once a fix is available,
we'll coordinate on disclosure timing with you before making the report
public.
