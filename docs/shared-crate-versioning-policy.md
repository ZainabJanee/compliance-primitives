# Versioning and Compatibility Policy for Shared Library Crates

This document states the compatibility commitments and rollout policy for
the shared library crates in this workspace: **`compliance-pausable`** and
**`compliance-check`** (once introduced). These crates are consumed as Rust
library dependencies by multiple contracts in the workspace; a breaking
change to either one has a wider blast radius than a change to any single
contract.

This policy extends the general storage-layout versioning rules in
[`STORAGE_VERSIONING.md`](../STORAGE_VERSIONING.md). Read that document
first — it applies to every contract in the workspace. This document adds
the extra policies specific to *shared library* crates.

---

## Scope

| Crate | Path | Role |
|-------|------|------|
| `compliance-pausable` | `contracts/pausable` | Compile-time pause/unpause helper linked into every primitive contract |
| `compliance-check` | `contracts/compliance-check` *(planned)* | Shared `ComplianceCheck` trait for unified cross-contract check interface |

Both crates are **library crates** — they have no `#[contract]` macro, emit
no wasm exports of their own, and are never deployed independently. However,
because they are compiled into multiple deployable contracts, a source
change to one of them implicitly changes every contract that depends on it.

---

## What counts as a breaking change in a shared library crate

For ordinary Rust libraries the Rust API compatibility guidelines
([the Rust API Guidelines](https://rust-lang.github.io/api-guidelines/))
apply. For *these* shared crates the bar is higher because consumers are
compiled Soroban contracts, not Rust applications that can simply
recompile:

### Breaking (requires lockstep bump of all dependents)

- **Removing or renaming a public function or type** — any contract
  that calls `compliance_pausable::require_not_paused(...)` will fail to
  compile after the function is removed or renamed.
- **Changing a public function's signature** — adding or removing
  parameters, changing a parameter type, changing the return type.
- **Changing the storage key written by the library** — `compliance-pausable`
  writes a `"Paused"` key into the calling contract's instance storage.
  Changing that key would leave existing deployed instances with a live
  `"Paused"` entry that the new code no longer reads, silently breaking
  pause state for every already-deployed primitive. Treat as a storage-
  layout breaking change per `STORAGE_VERSIONING.md`.
- **Changing the error type or error variant that the library returns** —
  callers pattern-match on these; a rename or reorder of an error enum
  variant is a breaking change at the ABI level.

### Non-breaking (no lockstep requirement)

- Adding a new public function, type, or trait implementation that does
  not change any existing public API.
- Changing a private implementation detail (a private helper function,
  an internal type, or the body of a function whose behavior is not part
  of the documented contract).
- Improving documentation, comments, or test coverage without touching
  public API.
- Adding a new `DataKey` variant to an internal enum, as long as it does
  not collide with existing keys (see `STORAGE_VERSIONING.md`).

---

## Rollout policy for breaking changes

### Option 1 — Lockstep bump (preferred for small, well-scoped changes)

1. Make the breaking change in the shared crate.
2. In the **same PR**, update every workspace crate that depends on the
   shared crate to compile cleanly with the new API.
3. Bump the shared crate's version (even pre-1.0; see below) in the
   workspace `Cargo.toml` so the change is traceable.
4. Call out the breaking change explicitly in the PR description and in
   `CHANGELOG.md`, listing every affected contract crate.

This is the right choice when the API change is small and every dependent
is in this workspace. Because all dependents are in the same Cargo workspace,
a single `cargo build --workspace` verifies the entire lockstep update
at once.

### Option 2 — Dual-version support (for larger or phased rollouts)

When the breaking change is large enough that updating all dependents in
one PR is impractical, the library can temporarily support both the old and
new API via a Cargo feature flag or a parallel function/type:

1. Add the new API alongside the old under an additive feature or a `v2`-
   suffixed name (e.g. `require_not_paused_v2`).
2. Migrate one dependent contract per PR, switching it to the new API and
   verifying it compiles and tests pass.
3. Once all dependents are migrated, remove the old API in a follow-up PR
   and drop the `v2` suffix (or feature flag).
4. The final removal PR bumps the version and records the change in
   `CHANGELOG.md`.

Use this option when more than two contract crates need changes, or when
the migration involves storage-layout changes that require per-contract
`UPGRADE.md` entries (per `STORAGE_VERSIONING.md`).

---

## Versioning scheme

Both shared crates track the workspace version (`workspace.package.version`
in the root `Cargo.toml`) rather than versioning independently, unless they
are published to crates.io.

**`compliance-pausable`** is publish-ready and has a dedicated crates.io
publishing workflow (`.github/workflows/publish-pausable.yml`). For this
crate:

- A **breaking API change** requires a version bump **before** the PR is
  merged, even pre-1.0. Downstream projects that pull `compliance-pausable`
  from crates.io will see the version number; a breaking change that ships
  under the same version number corrupts their builds silently.
- A **non-breaking addition** may ship in a patch or minor bump at the
  maintainer's discretion.
- Follow the publishing process in
  [`contracts/pausable/README.md`](../contracts/pausable/README.md)
  (push a `pausable-v<version>` tag, or trigger the workflow manually)
  *after* the lockstep PR has landed.

**`compliance-check`** (planned) — when introduced, follow the same rules
as `compliance-pausable`. If published to crates.io, it must have its own
independent version and publishing workflow.

---

## Documentation requirements

Every PR that makes a breaking change to a shared library crate must
include:

1. An update to this document's [Breaking change log](#breaking-change-log)
   with the date, the crate, the change, and the set of dependent contracts
   updated in lockstep.
2. An update to `CHANGELOG.md` at the workspace level, under the
   corresponding version bump, listing the affected contracts.
3. If the change touches storage keys written by the library: an entry in
   `STORAGE_VERSIONING.md` or the affected contract's `UPGRADE.md`,
   per the rules in that document.

Non-breaking additions require a `CHANGELOG.md` entry only if they add
new public API surface (new functions, traits, or types that callers can
start depending on).

---

## Breaking change log

*This section is updated by each PR that makes a breaking change to a
shared library crate.*

| Date | Crate | Change | Dependent contracts updated |
|------|-------|--------|-----------------------------|
| *(none yet)* | | | |

---

## For contributors

Before making a change to `contracts/pausable/src/` or
`contracts/compliance-check/src/`:

1. Read [What counts as a breaking change](#what-counts-as-a-breaking-change-in-a-shared-library-crate) above.
2. If the change is breaking, choose [Option 1 or Option 2](#rollout-policy-for-breaking-changes) and note your choice in the PR description.
3. Run `cargo build --workspace` and `cargo test --workspace` locally to
   confirm all dependents compile and pass before opening the PR.
4. If `compliance-pausable` is affected and the change is breaking, bump
   its version before pushing and follow the publishing steps.

When in doubt, treat the change as breaking and discuss in the PR — it's
easier to downgrade a breaking classification than to retroactively fix
a silent storage or API breakage across live deployments.
