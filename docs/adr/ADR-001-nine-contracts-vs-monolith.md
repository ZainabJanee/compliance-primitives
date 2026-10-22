# ADR-001: Nine Separate Contracts Instead of One Monolith

**Status**: Accepted  
**Date**: 2024-01-01  
**Deciders**: stellar-compliance-kit maintainers  
**Relates to**: [ARCHITECTURE.md](../../ARCHITECTURE.md), issues discussing workspace consolidation

---

## Context

When designing `compliance-primitives`, the team had to decide on the workspace
topology: one large contract that handles denylist, allowlist, jurisdiction, audit
logging, circuit-breaking, multisig admin, and policy aggregation under a single
`#[contract]` macro, or nine independent, purpose-built contracts that compose via
cross-contract call.

This question will come up again whenever someone proposes consolidating two or
more contracts to save on deployment overhead, reduce ledger entries, or
simplify the integration surface. This record documents the reasoning so those
future discussions start from a shared baseline rather than re-litigating first
principles each time.

### Forces at play

- Soroban charges for cross-contract invocations. Every additional hop in the
  call graph costs CPU and fee instructions.
- An issuer deploying this stack today has to deploy, configure, and wire up
  multiple contract addresses instead of one.
- Auditing nine separate contracts is operationally more work than auditing one
  (more scope, more artifacts, more upgrade events to monitor).
- On the other hand, smart-contract audits charge by audit surface — a 3,000-line
  monolith costs more to audit per-change than a 200-line focused contract does.
- Soroban has no inheritance; the only composition mechanisms are cross-contract
  call (runtime) or a shared library compiled into each consumer (compile time,
  like `compliance-pausable`).

---

## Decision

**We maintain nine separate contract crates — one per compliance primitive — and
allow composition only via cross-contract call or compile-time library inclusion.**

No contract in this workspace contains another contract's business logic inline
(outside the `pausable` library special case described below).

---

## Rationale

### 1. Audit surface scales with the contract, not the workspace

A security audit of a smart contract is an audit of *everything it can do*. A
monolith that handles allowlists, denylists, jurisdiction checks, circuit-breaking,
multisig authorization, and audit logging has a combined blast radius for every
bug found anywhere in it. A reviewer auditing `denylist-gate` needs to reason
about only ~200 lines covering one concern; they do not have to also reason about
how a jurisdiction-check edge case might interact with the multisig-admin path.

In practice this means:

- A bug in `multisig-admin`'s threshold logic cannot affect `denylist-gate` in
  any way — they share no code and no storage namespace.
- A re-audit of one contract after a change does not require re-auditing unrelated
  contracts in the same deployment.
- Community audit contributors can scope a focused review without reading the
  entire workspace.

### 2. Independent upgrade paths

Each deployed contract can be upgraded independently. If `jurisdiction-flag` needs
a new storage key (see [`jurisdiction-flag/UPGRADE.md`](../../contracts/jurisdiction-flag/UPGRADE.md)),
that upgrade can go out without touching the `denylist-gate` or `circuit-breaker`
deployments. In a monolith, every upgrade to any piece of logic forces a
redeployment of the whole contract and a re-audit of the whole upgrade path.

For an issuer running a live stablecoin with real balances, "upgrade only the
component that changed" is a materially lower-risk operation than "upgrade the
entire compliance contract."

### 3. Opt-in composition — issuers deploy only what they need

Not every issuer needs every primitive. A simple stablecoin issuer might need
only `denylist-gate` and `circuit-breaker`. A full RWA issuer might add
`jurisdiction-flag`, `compliance-aggregator`, and `multisig-admin`. Because each
primitive is independently deployable, an issuer pays only for what they use — in
ledger entries, in deployment cost, and in ongoing TTL rent.

A monolith would force every issuer to carry the dead weight of functionality
they don't use, pay to deploy it, and accept the wider audit surface regardless.

### 4. Composability is the product, not a cost

These contracts are designed to be composed into an issuer's own token or RWA
wrapper. `denylist-gate` exposes `check(address)` so any caller — not only the
composition contracts in this workspace — can call it. `jurisdiction-flag` exposes
`is_permitted_jurisdiction(address, allowed_codes)`. These are stable, typed
interfaces that callers can depend on, test against, and reason about in isolation.

A monolith would collapse these interfaces into internal function calls: no stable
cross-contract boundary, no way for an external caller to call just one check
without going through the whole contract's routing logic, no way to unit-test one
check without exercising the monolith's dispatch layer.

### 5. The cross-contract call cost is real but bounded and predictable

Cross-contract invocation on Soroban does add CPU instruction cost. This is not
ignored; it is accepted and managed:

- `compliance-aggregator` and `policy-engine` exist specifically to batch multiple
  primitive checks into a single call from the consumer's perspective — an issuer
  who wants `denylist-gate + jurisdiction-flag` in one round-trip uses
  `compliance-aggregator` rather than calling each primitive separately.
- `circuit-breaker.is_frozen()` is a single bool read, cheap enough that its
  cross-contract overhead is negligible relative to the transfer it gates.
- Observed instruction budgets for the full composed path are documented in
  [`BENCHMARKS.md`](../../BENCHMARKS.md) so teams can evaluate the real cost rather
  than relying on intuition.

The overhead is bounded, predictable, and buyable at deploy time — unlike the
audit-surface and upgrade risk of a monolith, which grow unboundedly with the
codebase.

### 6. The `pausable` library: the one deliberate exception

`compliance-pausable` (`contracts/pausable`) is compiled directly into each
contract that needs it rather than called via cross-contract invocation. This
looks like a contradiction but is not:

- `pausable` has no `#[contract]` macro and exports no wasm functions — it is
  *not* an independent deployable contract. It is a `#![no_std]` helper library.
- The alternative — extracting a `#[contract]` crate and calling it via
  cross-contract call — would link that crate's wasm exports into every dependent
  contract's binary (linker collision) and add a cross-contract round-trip for
  the most common operation (`require_not_paused`), which every public function
  in the contract calls before doing anything.
- Compiling the same 50-line helper into multiple contracts is not a composability
  violation — it is a build-time deduplication decision. Each contract's pause
  state is stored in that contract's own instance storage and is entirely
  independent; there is no shared state.

See [`docs/pausable-design.md`](../pausable-design.md) for the full reasoning.

---

## Alternatives considered

### A. Full monolith — one contract, all logic

**Pros**: Single deployment, no cross-contract call overhead, single admin key.

**Cons**: Entire compliance stack must be re-audited on every change to any
component. Any bug's blast radius is the entire stack. No opt-in — all issuers
carry all logic. No independent upgrade paths. Rejected.

### B. Two contracts — one for checks, one for admin/control

**Pros**: Reduces call hops while separating admin logic.

**Cons**: Still couples unrelated check types (denylist, jurisdiction, allowlist)
into one contract. Does not solve independent upgrade paths. Audit surface for the
check contract is still the union of all check types. Not meaningfully better than
option A for the problems we care about. Rejected.

### C. Nine contracts but with a shared storage namespace (single ledger entry)

**Pros**: Reduces ledger entry count.

**Cons**: Shared storage creates cross-contract coupling at the data layer.
`denylist-gate` reading from storage keys that `jurisdiction-flag` also writes
reintroduces exactly the blast-radius and audit-surface problem we split the
contracts to avoid. Rejected.

### D. Current decision: nine independent contracts, compose via cross-contract call

**Accepted.** See [Rationale](#rationale) above.

---

## Consequences

### Positive

- Each contract's audit scope stays small and focused.
- Issuers can deploy and upgrade individual primitives independently.
- The cross-contract interface for each primitive is stable and independently testable.
- Adding a new primitive doesn't force re-audit of existing ones.
- External developers can compose these primitives in ways we haven't anticipated.

### Negative / accepted costs

- A fully-composed deployment involves more ledger entries and more transactions
  to configure.
- Cross-contract calls add instruction overhead. Mitigated by `compliance-aggregator`
  and `policy-engine` for the common batched-check case.
- Documentation complexity: nine contracts means nine `SPEC.md` sections, nine
  interface docs, and multiple upgrade paths to maintain.

### Future

If a future proposal suggests merging two or more contracts, it should address
the following questions before the merge is accepted:

1. **Audit surface**: does the merged contract's audit scope grow beyond what a
   focused review of one compliance concern requires? Who will re-audit?
2. **Upgrade independence**: will every issuer using either of the merged contracts
   be forced to accept a combined upgrade with a larger blast radius?
3. **Opt-in composition**: will issuers who need only one of the two components
   now be forced to deploy both?
4. **Interface stability**: will callers of the individual contract interfaces need
   to change their integration?

If any of these questions doesn't have a satisfying answer, the merge should be
rejected and this ADR updated to record the outcome.
