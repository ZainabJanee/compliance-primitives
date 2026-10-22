# compliance-primitives

On-chain compliance primitives for Stellar — allowlist/denylist token
gating and jurisdiction checks for RWA and stablecoin issuers, built on
Soroban.

[![CI](https://github.com/stellar-compliance-kit/compliance-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/stellar-compliance-kit/compliance-primitives/actions/workflows/ci.yml)

## The problem

RWA and stablecoin issuers building on Stellar/Soroban all end up needing
the same handful of compliance gates — allowlists, denylists, jurisdiction
restrictions — before they can legally let a token move. Today every issuer
hand-rolls this logic inside their own token contract, which means more
audit surface, more chances for a subtle bug to become a compliance
incident, and no shared, reviewed reference to build on. `compliance-primitives`
exists to give the ecosystem a small set of standardized, auditable,
independently testable contracts for exactly these checks.

## What's in this repo

This is a Cargo workspace with nine focused Soroban contracts, each doing
one job:

| Contract | Description |
| --- | --- |
| [`allowlist-token`](./contracts/allowlist-token) | Token wrapper that only permits transfers between two addresses that are both present on an on-chain allowlist. Admin-managed; forwards cleared transfers to an underlying SEP-41 token contract. |
| [`denylist-gate`](./contracts/denylist-gate) | Standalone denylist other contracts call via cross-contract invocation (`check(address)`) before executing a transfer. Not a token itself — meant to be composed. |
| [`jurisdiction-flag`](./contracts/jurisdiction-flag) | Attaches an issuer-controlled jurisdiction code (e.g. an ISO country code) to an address, with a helper (`is_permitted_jurisdiction`) other contracts can call to check an address against a permitted-jurisdictions list. |
| [`audit-log`](./contracts/audit-log) | On-chain, append-only audit trail for compliance events emitted by other primitives, queryable by other contracts at invocation time. |
| [`circuit-breaker`](./contracts/circuit-breaker) | Admin-controlled emergency freeze switch other contracts can check before allowing a transfer. |
| [`compliance-aggregator`](./contracts/compliance-aggregator) | Batches `denylist-gate` and `jurisdiction-flag` checks into a single cross-contract call, returning an aggregated pass/fail plus a per-check breakdown. |
| [`multisig-admin`](./contracts/multisig-admin) | M-of-N multisig authorization, usable as the `admin`/`issuer` address of any of the other primitives via Soroban's custom-account interface. |
| [`pausable`](./contracts/pausable) | Shared pause/unpause helper library composed into the other contracts at compile time. Not a deployable contract itself — no `#[contract]` macro or wasm exports. |
| [`policy-engine`](./contracts/policy-engine) | Composes multiple compliance checks (denylist, jurisdiction) with configurable AND/OR logic into a single `evaluate` call. |

[`/examples/denylist-gate-consumer`](./examples/denylist-gate-consumer) is a
minimal reference token showing the cross-contract calling pattern for
`denylist-gate`. [`/examples/rwa-token`](./examples/rwa-token) composes all
three primitives in one `transfer` path — see its
[TESTNET.md](./examples/rwa-token/TESTNET.md) for the testnet reference
deployment and walkthrough.

### Examples

Each example under `/examples` demonstrates a different composition pattern:

- **[circuit-breaker-policy-engine](./examples/circuit-breaker-policy-engine)** — wires `circuit-breaker` as a pre-check before `policy-engine` evaluation
- **[denylist-gate-consumer](./examples/denylist-gate-consumer)** — minimal token calling `denylist-gate` and `circuit-breaker` before transfers
- **[denylist-gate-sep41](./examples/denylist-gate-sep41)** — `denylist-gate` integration for SEP-41 anchor compliance
- **[jurisdiction-flag-consumer](./examples/jurisdiction-flag-consumer)** — token enforcing jurisdiction-based transfer restrictions
- **[jurisdiction-denylist-consumer](./examples/jurisdiction-denylist-consumer)** — combines `jurisdiction-flag` and `denylist-gate` checks
- **[multisig-governed-allowlist](./examples/multisig-governed-allowlist)** — `multisig-admin` as the admin of `allowlist-token`, so every allowlist change requires M-of-N signer approval
- **[rwa-compliance-flow](./examples/rwa-compliance-flow)** — full RWA compliance stack with allowlist, denylist, and jurisdiction checks
- **[rwa-token](./examples/rwa-token)** — reference RWA token composing all three primitives (testnet deployment available)

## Quick start

**New here?** Start with [**QUICKSTART.md**](./QUICKSTART.md) — it walks you through clone → build → test → example → testnet → web playground in one go.

For a condensed reference, the essentials are:

```sh
# Clone
git clone https://github.com/stellar-compliance-kit/compliance-primitives.git
cd compliance-primitives

# Run the full test suite (all three contracts + example)
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Build a contract to wasm (the wasm32v1-none target is pinned in
# rust-toolchain.toml, so rustup installs it automatically)
stellar contract build

# Equivalent, via the cargo alias in .cargo/config.toml
cargo build-wasm

# Deploy a built contract to testnet, e.g. denylist-gate
stellar contract deploy \
  --wasm target/wasm32v1-none/release/denylist_gate.wasm \
  --source <your-testnet-identity> \
  --network testnet
```

See [`examples/testnet.env.example`](./examples/testnet.env.example) for an
example config covering the identity/network setup needed for the deploy +
invoke round trip above.

Alternatively, use [`scripts/deploy-testnet.sh`](./scripts/deploy-testnet.sh) to
build all contracts and deploy one to testnet in a single step:

```sh
STELLAR_SOURCE=<your-testnet-identity> ./scripts/deploy-testnet.sh denylist-gate
```

To deploy and wire up **all eight deployable contracts** in the workspace
in one step, see
[`scripts/deploy-full-stack-testnet.sh`](./scripts/deploy-full-stack-testnet.sh)
and its walkthrough,
[`docs/full-stack-testnet-walkthrough.md`](./docs/full-stack-testnet-walkthrough.md).

### Working with a single contract

You don't need to build or test the whole workspace to work on one
contract. Each crate can be built and tested in isolation with Cargo's
`-p <crate>` flag, using the crate name from its `Cargo.toml`
(`allowlist-token`, `denylist-gate`, or `jurisdiction-flag`):

```sh
# Test just one contract
cargo test -p allowlist-token

# Lint just one contract
cargo clippy -p denylist-gate --all-targets -- -D warnings

# Build just one contract to wasm
cargo build -p jurisdiction-flag --target wasm32v1-none --release
```

## Architecture note

These contracts are building blocks, not standalone products. They're
meant to be composed into a real token or RWA wrapper contract via
cross-contract calls — for example, a token's `transfer` function calling
`denylist-gate.check(address)` before moving funds, or checking
`jurisdiction-flag.is_permitted_jurisdiction(address, allowed_codes)`
before permitting a transaction. See
[`/examples/denylist-gate-consumer`](./examples/denylist-gate-consumer) for
a worked example of that pattern. `allowlist-token` is the one exception —
it's a thin wrapper you can deploy in front of an existing SEP-41 token —
but even it delegates the real transfer to the underlying token contract
rather than reimplementing token logic itself.

## Choosing which primitives to compose

With nine contracts now available, the decision of which primitives to use depends on your use case.
See **[docs/PRIMITIVE_SELECTION_GUIDE.md](./docs/PRIMITIVE_SELECTION_GUIDE.md)** for profiles
of common RWA and stablecoin use cases, and which primitive combinations fit each.

## Migrating from hand-rolled compliance

If you already have allowlist, denylist, or jurisdiction checks baked into
your own token contract, see **[docs/MIGRATION.md](./docs/MIGRATION.md)** for
a step-by-step guide to replacing them with these primitives — including
mapping your existing checks, deployment & wiring, data backfill, and rollback.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the fork → branch → PR flow,
and [GOVERNANCE.md](./GOVERNANCE.md) for how issues are triaged and labeled.
This repo is part of the **Drips Wave Stellar Program**, and issues are
labeled by complexity (`complexity: trivial`, `complexity: medium`,
`complexity: high`) so you can find something that matches how deep you
want to go — issues tagged `good first issue` are a good place to start.

## Security

See [SECURITY.md](./SECURITY.md) for how to report a vulnerability.

## License

[MIT](./LICENSE)
