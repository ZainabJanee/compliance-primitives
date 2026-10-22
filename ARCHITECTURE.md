# Architecture

This document describes how the nine contract crates under `contracts/` fit
together: what each one is for, which category it falls into, and which
other contracts it calls into or is called by. For the security invariants
and threat model of each contract, see [`SPEC.md`](SPEC.md). For the on-chain
event schema, see [`EVENTS.md`](EVENTS.md).

## Compose, don't inherit

Every contract here is a small, independently deployable Soroban contract.
None of them subclass or extend another contract's code — Soroban has no such
mechanism, and it wouldn't be desirable even if it did, since each contract's
audit surface would then include every ancestor's logic. Instead, contracts
compose by **cross-contract call**: contract A stores contract B's address
and invokes it at call time through a `#[contractclient]`-generated client,
exactly the way any external caller would. The one exception is
[`pausable`](#shared-library-pausable), a plain Rust library with no
`#[contract]` macro and no wasm exports of its own, which is compiled
**into** the contracts that use it rather than called at runtime — see
below for why that one case doesn't violate compose-not-inherit.

## The three categories

```
                     ┌─────────────────────────────┐
                     │      CONTROL CONTRACTS       │
                     │  multisig-admin · circuit-breaker │
                     └──────────────┬───────────────┘
                                    │ set as admin address / checked first
                                    ▼
   ┌───────────────────────────────────────────────────────────┐
   │                   COMPOSITION CONTRACTS                     │
   │           compliance-aggregator · policy-engine             │
   └───────────────┬───────────────────────────┬─────────────────┘
                   │ cross-contract call        │ cross-contract call
                   ▼                             ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                     PRIMITIVE CONTRACTS                      │
   │  denylist-gate · allowlist-token · jurisdiction-flag · audit-log │
   └─────────────────────────────────────────────────────────────┘
                   ▲
                   │ linked in at compile time (no wasm export, no call)
                   │
              pausable (shared library, not independently deployed)
```

### Primitive contracts

Standalone, independently deployable building blocks. Each does one
compliance-related job and is designed to be called into by a caller's own
token or wrapper contract (or, in `allowlist-token`'s case, deployed in
front of one). None of the four depends on any other contract in this
workspace at the crate level.

| Contract | Job | Called by |
|---|---|---|
| `denylist-gate` | `check(address) -> bool` — is this address blocked? | consumer contracts, `compliance-aggregator`, `policy-engine` |
| `allowlist-token` | Wraps a SEP-41 token; only forwards `transfer` between two allowlisted addresses | end users / wallets directly (it's the one primitive meant to sit in the call path itself, not be queried by another contract) |
| `jurisdiction-flag` | `is_permitted_jurisdiction(address, allowed_codes) -> bool` | consumer contracts, `compliance-aggregator`, `policy-engine` |
| `audit-log` | Append-only on-chain event trail: `record(source, kind, subject, detail)` | opt-in — a primitive that has been wired with `set_audit_log(admin, audit_log_id)` calls it after each mutating operation. Currently wired into `denylist-gate`'s design and `circuit-breaker`'s `freeze`/`unfreeze`; not yet wired into `allowlist-token` or `jurisdiction-flag` |

`audit-log` is grouped with the primitives rather than the composition
contracts because it doesn't aggregate *checks* — it's a standalone,
independently useful building block (an on-chain event ledger) that any
contract, primitive or otherwise, can opt into.

### Control contracts

Sit *above* the primitives operationally: they don't perform compliance
checks themselves, they govern *who can operate* the primitives or provide
an emergency stop that consumers check before trusting any check result.

| Contract | Job | Relationship to primitives |
|---|---|---|
| `multisig-admin` | M-of-N multisig authorization via Soroban's `CustomAccountInterface` | Deployed once, then its contract address is passed as the `admin`/`issuer` address to any primitive at `initialize` time. From that point, every `admin.require_auth()` call inside the primitive transparently re-enters `multisig-admin.__check_auth`, requiring `threshold` signer approvals instead of a single key. No changes to the primitives are needed — they already accept any `Address` as admin. See [`DESIGN_MULTISIG_ADMIN.md`](DESIGN_MULTISIG_ADMIN.md). |
| `circuit-breaker` | Single shared `is_frozen()` emergency switch | Not called *by* the primitives themselves — it's checked *by consumer contracts*, before any primitive check, as the fastest possible fail-closed gate during an incident (freeze once, every gated transfer stops, no per-primitive pause calls needed). See [`docs/emergency-freeze-design.md`](docs/emergency-freeze-design.md). Distinct from each primitive's own local `pause`/`unpause` (via the `pausable` library), which is a slower, per-contract, admin-controlled switch rather than a single incident-wide one. |

### Composition contracts

Reduce the cross-contract-call overhead and boilerplate a consumer would
otherwise pay for combining multiple primitive checks into one compliance
decision. Both call into the primitive layer; neither calls the other.

| Contract | Job | Composes |
|---|---|---|
| `compliance-aggregator` | Registers at most one `denylist-gate` and one `jurisdiction-flag` address; `check_address`/`check_all` AND-combine their results into a single call with a per-check breakdown | `denylist-gate.check`, `jurisdiction-flag.is_permitted_jurisdiction` |
| `policy-engine` | Registers an arbitrary, admin-managed list of `CheckKind` entries (each naming a target contract + params) plus a `CombineOp` (`All` = AND, `Any` = OR); `evaluate(from, to)` runs every check against both addresses | `denylist-gate.check`, `jurisdiction-flag.is_permitted_jurisdiction` (same two primitive interfaces as the aggregator, via its own generated clients) |

The two are deliberately not merged. `compliance-aggregator` is the simpler,
fixed-shape tool (always AND, always these two checks) for the common case;
`policy-engine` trades that simplicity for a configurable check list and
AND/OR combination logic. A caller that outgrows the aggregator's fixed
shape moves to the policy engine rather than the aggregator growing a second
mode. See the "Relationship to #109" note in `compliance-aggregator`'s
module doc comment for the original design discussion.

Neither composition contract currently calls into `allowlist-token` — both
treat it as the deploy-in-front-of-a-token primitive that consumers use
directly, not as a queryable check.

### Shared library: `pausable`

`contracts/pausable` (crate `compliance-pausable`) is not one of the above
three categories — it has no `#[contract]` macro, exports no wasm functions,
and is never deployed on its own. It's a small `#![no_std]` helper
(`is_paused`, `pause`, `unpause`, `require_not_paused`) that gets compiled
directly into each primitive that depends on it, storing a `bool` under a
fixed `"Paused"` key in *that primitive's own* instance storage. This is the
one deliberate exception to "compose by cross-contract call": identical
pause/unpause logic duplicated three times would triple the audit surface
for zero behavioral benefit, and — unlike the primitive contracts — there's
no reason a caller would ever need to invoke this logic on an address other
than its own. See [`docs/pausable-design.md`](docs/pausable-design.md) for
the full rationale, including why depending on another `#[contract]` crate
directly (instead of extracting a library) was rejected — it would link that
crate's wasm exports into the depending contract's binary and collide at the
linker.

`allowlist-token` and `jurisdiction-flag` depend on and use this crate today.
`denylist-gate` depends on it in `Cargo.toml` but does not yet call it from
`src/lib.rs` — that wiring is incomplete pending a separate fix (tracked
outside this issue's scope; see the note in the PR that introduced this
document for the current compile status of each contract).

## Putting it together: a fully-composed deployment

A deployer who wants every control in this workspace active at once would
end up with a call graph like:

```
consumer's token contract
  transfer(from, to, amount)
    → circuit-breaker.is_frozen()                          (control: fail fast if frozen)
    → policy-engine.evaluate(from, to)                      (composition)
        → denylist-gate.check(from)                         (primitive)
        → denylist-gate.check(to)
        → jurisdiction-flag.is_permitted_jurisdiction(to, allowed)
        → [denylist-gate.record(...) via audit-log, if wired]
    → proceed with transfer, or reject
```

...where `denylist-gate`'s own `admin` address is `multisig-admin`'s
contract address (control), so that any future denylist mutation itself
requires M-of-N sign-off rather than trusting a single key. This mirrors
[`examples/denylist-gate-consumer`](examples/denylist-gate-consumer) and
[`examples/jurisdiction-denylist-consumer`](examples/jurisdiction-denylist-consumer),
extended with the six newer contracts; neither example currently wires in
every optional contract above, since not every deployment needs all of
them — a consumer picks the primitives, control contracts, and composition
layer that match its own risk model.
