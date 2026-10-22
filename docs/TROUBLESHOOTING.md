# Troubleshooting — common local build failures

This guide covers failure modes that have come up during workspace development
and merge-conflict resolution.  Each section describes what the error looks
like, why it happens, and the exact fix.

---

## 1. Missing workspace member

**Error:**

```
error: no such package
  → could not find `my-new-crate` at `contracts/my-new-crate`
```

or, when running `cargo check --workspace`:

```
error[E0432]: unresolved import `my_new_crate`
```

**Why it happens:**

Every crate under `contracts/` and `examples/` must be listed under
`[workspace.members]` in the root `Cargo.toml`.  A common pattern during
branching is to create the directory and `Cargo.toml` for a new crate but
forget to add it to the workspace manifest.

**Fix:**

Open the root `Cargo.toml` and add the missing path to `[workspace.members]`:

```toml
[workspace]
members = [
  "contracts/allowlist-token",
  "contracts/denylist-gate",
  # ... existing members ...
  "contracts/my-new-crate",   # ← add this
]
```

Then verify with:

```sh
cargo check --workspace
```

You can also run `./scripts/check-workspace-members.sh` which cross-checks
every directory under `contracts/` and `examples/` against the manifest.

---

## 2. Package-name / dependency-name mismatch in `[workspace.dependencies]`

**Error:**

```
error: package `my-new-crate` is listed as a workspace dependency,
       but it does not appear in `[workspace.members]`
```

or:

```
error: failed to select a version for `my_new_crate`
       ... package `my_new_crate` was not found in the registry
```

**Why it happens:**

Cargo distinguishes between the **package name** (the `name` field in the
crate's own `Cargo.toml`) and the **directory name**.  In `[workspace.dependencies]`
the key must match the package name exactly — including whether it uses hyphens
or underscores — not the directory path.  A mismatch causes Cargo to look for
the package on crates.io instead of resolving it locally.

**Example of the bug:**

```toml
# Root Cargo.toml — WRONG: directory is contracts/my-new-crate but
# the crate's Cargo.toml declares name = "my_new_crate" (underscore)
[workspace.dependencies]
my-new-crate = { path = "contracts/my-new-crate", version = "0.1.0" }
```

**Fix:**

Check the `name` field in the crate's own `Cargo.toml`:

```toml
# contracts/my-new-crate/Cargo.toml
[package]
name = "my_new_crate"   # ← this is the canonical name
```

Then match it in `[workspace.dependencies]`:

```toml
# Root Cargo.toml — CORRECT
[workspace.dependencies]
my_new_crate = { path = "contracts/my-new-crate", version = "0.1.0" }
```

And wherever the crate is used as a dependency:

```toml
# contracts/some-consumer/Cargo.toml
[dependencies]
my_new_crate = { workspace = true }
```

---

## 3. `#![no_std]` contract tests using `std::fs` or `std::env`

**Error (in a `#![no_std]` contract crate):**

```
error[E0433]: failed to resolve: use of unresolved import `std`
  → use std::env;
  |        ^^^ use of undeclared crate or module `std`
```

or:

```
error[E0432]: unresolved import `std::fs`
```

**Why it happens:**

Soroban contracts are compiled with `#![no_std]` because the Wasm target
(`wasm32v1-none`) has no OS-level standard library.  Test modules inside the
same crate (`#[cfg(test)] mod test;`) inherit the `no_std` attribute, so any
test helper that calls `std::fs::read_to_string`, `std::env::var`, or similar
won't compile.

**Fix — option A (preferred): add `extern crate std` inside the test module**

```rust
// contracts/my-contract/src/test.rs
#[cfg(test)]
mod test {
    extern crate std;          // ← re-introduce std for the test module only
    use std::env;
    use std::fs;
    // ...
}
```

The `extern crate std` declaration is valid under `#![no_std]` inside a
`#[cfg(test)]` block because the test binary is built by the host toolchain
(not the Wasm target), which does have std available.

**Fix — option B: move test helpers to a separate `tests/` integration-test crate**

```
contracts/my-contract/
  src/
    lib.rs          # no_std contract
  tests/
    integration.rs  # plain Rust, has std implicitly
```

Integration tests in the `tests/` directory are compiled as separate crates
that do not inherit `#![no_std]`, so `use std::fs` works without any extra
declaration.  This is the pattern used by `contracts/pausable/tests/`.

**Note:** the `soroban_sdk::testutils` helpers (e.g. `Env::default()`,
`Address::generate`) work in both options because they are conditionally
compiled for tests in soroban-sdk itself.

---

## 4. Multiple errors at once after merging two feature branches

**Symptom:** `cargo check --workspace` reports five or more independent errors
across different contract crates simultaneously after merging two branches that
both touched shared files (e.g. `contracts/allowlist-token/src/lib.rs`).

**Why it happens:**

Git's three-way merge can leave syntactically valid but semantically broken
Rust — for example, a function body duplicated from both branches, or an
import that one branch removed but the other still uses.  `cargo check` finds
all errors in a single pass, making the output look worse than it is.

**Fix:**

1. Identify which crate each error belongs to (the `→` path at the top of
   each error group).
2. Fix one crate at a time and verify with `-p`:
   ```sh
   cargo check -p allowlist-token
   cargo test  -p allowlist-token
   ```
3. Once individual crates are clean, run the full workspace check:
   ```sh
   cargo check --workspace
   cargo test  --workspace
   ```

See also the [PR merge checklist](../GOVERNANCE.md#merging-conflicting-contract-prs)
for guidance on avoiding this state in the first place.

---

## 5. `wasm32v1-none` target not installed

**Error:**

```
error[E0463]: can't find crate for `core`
  = note: the `wasm32v1-none` target may not be installed
```

**Why it happens:**

The workspace pins a specific Rust toolchain via `rust-toolchain.toml`.
`rustup` installs the toolchain automatically on first use, but the
`wasm32v1-none` component may not be added if `rustup` was not used to
install the toolchain.

**Fix:**

```sh
rustup target add wasm32v1-none
```

Or, if the toolchain itself is missing:

```sh
rustup toolchain install $(cat rust-toolchain.toml | grep channel | cut -d'"' -f2)
rustup target add wasm32v1-none
```

Then verify a WASM build works:

```sh
stellar contract build
# or: cargo build -p denylist-gate --target wasm32v1-none --release
```

---

## 6. `cargo clippy` reports errors only on CI

**Symptom:** `cargo clippy --workspace --all-targets -- -D warnings` passes
locally but fails in CI with warnings-as-errors.

**Why it happens:**

Clippy lint levels can vary between Rust versions.  The workspace pins a
toolchain in `rust-toolchain.toml`; if your local `rustup` override is
different, you may see a different lint set.

**Fix:**

Make sure you are using the pinned toolchain:

```sh
rustup override set $(cat rust-toolchain.toml | grep channel | cut -d'"' -f2)
cargo clippy --workspace --all-targets -- -D warnings
```

If the lint is in code you did not touch (e.g. a generated file), suppress it
with a targeted `#[allow(...)]` rather than a global `#![allow(...)]`.

---

## Still stuck?

- Run `./scripts/check-workspace-members.sh` to catch membership/path mismatches.
- Check `CI.md` for the exact commands CI runs — reproducing them locally is
  the fastest way to find environment differences.
- Open an issue and paste the full `cargo check` output.
