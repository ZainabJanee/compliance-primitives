# compliance-pausable

Shared pause/unpause helper for Soroban compliance contracts.

This crate is **not a contract** — it has no `#[contract]` macro and
contributes zero wasm exports of its own. It's ordinary `#![no_std]` Rust
that gets compiled into each consuming contract's binary, giving
`denylist-gate`, `jurisdiction-flag`, and `allowlist-token` (and any future
primitive) identical pause semantics without duplicating the logic in each
crate.

## Why a shared crate

All three original primitives need the same pause/unpause/is_paused
behavior. Duplicating it per-contract adds audit surface with no benefit.
Because this crate emits no `#[contract]`-generated wasm exports, depending
on it from multiple contract crates doesn't cause the linker symbol
collisions that depending directly on another *contract* crate would.

## Storage layout

Pause state is a `bool` under the fixed instance-storage key `"Paused"` in
the **calling** contract's own storage — this crate never manages storage of
its own. See the crate's rustdoc (`cargo doc -p compliance-pausable --open`)
for the full rationale.

## Usage

```rust
use soroban_sdk::{contracterror, contractevent};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    // ...
    ContractPaused = 4,
}

#[contractevent]
pub struct Paused;

#[contractevent]
pub struct Unpaused;

// In a state-mutating method:
compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
```

`pause()`, `unpause()`, and `is_paused()` are exposed for the consuming
contract to wire up its own admin-gated `pause`/`unpause`/`is_paused`
methods and event emission — see `denylist-gate` or `jurisdiction-flag` for
a worked example.

## Publishing to crates.io

This crate is publish-ready (`description`, `license`, `repository`, and
`readme` metadata are set in `Cargo.toml`) and is published via a
manually- or tag-triggered workflow
(`.github/workflows/publish-pausable.yml`), **not** automatically on every
merge to `main`.

**Decision:** publish is opt-in, triggered by pushing a `pausable-v*` tag
(or running the workflow manually), rather than tied to the workspace's
overall release cadence. Rationale:

- The other crates in this workspace (`denylist-gate`, `allowlist-token`,
  `policy-engine`, etc.) are Soroban *contracts* — they're consumed as
  compiled wasm, not as Rust library dependencies, so publishing them to
  crates.io wouldn't serve their actual consumers. `compliance-pausable` is
  the one crate here meant to be pulled in as an ordinary Rust dependency by
  other Soroban projects, which is why it's the one being published.
- Tying publication to its own tag (rather than "every push to `main`
  publishes a new version") avoids bumping a public, semver-tracked crate
  for internal refactors that don't change its API, and gives whoever cuts
  the release a chance to write real release notes.
- If this crate's audience turns out to be "just this workspace," the
  workflow can be removed without touching the crate's Cargo.toml — it was
  kept decoupled from the crate itself specifically so that decision is
  reversible.

To cut a release: bump `version` in this crate's section of the workspace
`Cargo.toml` (or a crate-local `version` override), then push a tag matching
`pausable-v<version>` (e.g. `pausable-v0.1.0`) or run the
**Publish pausable to crates.io** workflow manually from the Actions tab.

## Scope of this decision: `compliance-pausable` only

The publishing decision above is **explicitly scoped to `compliance-pausable`**.
It does not automatically extend to any other shared crate that may be added
to this workspace (e.g. a future `compliance-check` trait crate).

### Framework for future shared crates

Each new workspace-internal crate should answer the same question independently:
*is this crate meant to be pulled in as an ordinary Rust library dependency by
projects outside this workspace?*

- **If yes** (the crate defines a shared trait, type, or helper that other
  Soroban projects compose with): follow the same model as
  `compliance-pausable` — set `publish = false` in `Cargo.toml` until the API
  is stable enough to commit to a semver contract, add `description`, `license`,
  `repository`, and `readme` metadata, create a dedicated publish workflow
  (`.github/workflows/publish-<crate-name>.yml`), and gate releases on an
  explicit tag rather than every merge to `main`.

- **If no** (the crate is a Soroban contract consumed as compiled wasm, or is
  purely workspace-internal scaffolding): leave `publish = false` permanently.
  Publishing a contract crate to crates.io would not serve its consumers, who
  pull in the compiled `.wasm` artifact, not the Rust source.

### `compliance-check` specifically

If `compliance-check` is added as a shared trait/interface crate (analogous to
`compliance-pausable`), the decision is: **it should follow the same
opt-in-publish model as `compliance-pausable`**, with its own versioned tag and
workflow, rather than being bundled into `compliance-pausable`'s release or
published on an implicit schedule.  The rationale is identical — external
Soroban projects may want to depend on the trait definition without pulling in
the full primitives workspace, the API needs room to stabilise before
committing to semver guarantees, and keeping the publish step explicit makes
the decision reversible if the audience turns out to be workspace-only.

If `compliance-check` is not intended for external consumption (e.g. it only
exists to reduce duplication within this repo), keep `publish = false` and
document that decision in its own `README.md` to prevent this question from
being re-litigated.
