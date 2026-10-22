# Soroban SDK Compatibility

This document records which `soroban-sdk` versions have been tested against
this workspace's full test suite, and notes the API-shape assumptions baked
into the contracts and tests that would need re-checking on any future SDK
version bump.

## Compatibility matrix

| soroban-sdk | Rust toolchain | Wasm target        | Status                  | Notes                              |
|-------------|----------------|--------------------|-------------------------|------------------------------------|
| **27.0.0**  | 1.91.0         | `wasm32v1-none`    | ✅ Tested — CI green    | Current pinned version             |

The pinned versions are enforced by:
- `Cargo.toml`: `soroban-sdk = "27.0.0"` (workspace dependency)
- `rust-toolchain.toml`: `channel = "1.91.0"`, `targets = ["wasm32v1-none"]`

## API-shape assumptions

The contracts and tests rely on the following soroban-sdk API shapes. Each
item notes when the shape was introduced/stabilised so reviewers know which
releases would be affected by a version bump.

### 1. Event publishing via `#[contractevent]` + `.publish(&env)`

All nine contracts emit events using the `#[contractevent]` macro with
`#[topic]` field annotations, and call `.publish(&env)` on the resulting
struct. Example from `jurisdiction-flag`:

```rust
#[contractevent]
pub struct JurisdictionSet {
    #[topic]
    pub address: Address,
    pub code: String,
}

JurisdictionSet { address, code }.publish(&env);
```

This pattern was introduced around soroban-sdk **~20.x**. Earlier SDKs used
`env.events().publish(topics, data)` directly. A bump to a version that
reverts or further changes event publishing would require updating every
`#[contractevent]` usage across all contracts.

### 2. Error codes via `#[contracterror]`

Contract error types are declared with `#[contracterror]` and derive `Copy,
Clone, Debug, Eq, PartialEq, PartialOrd, Ord`, mapping to `u32` variants
returned from contract functions. This macro has been stable since early SDK
versions but its derive requirements may change.

### 3. Storage with `extend_ttl`

Persistent storage entries use TTL management:

```rust
env.storage().persistent().extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
```

The `extend_ttl` API on `Persistent` storage was stabilised in soroban-sdk
**~20.x** alongside the introduction of state expiry. Earlier versions had a
different TTL management interface.

### 4. `address.require_auth()`

All auth-gated entry points call `address.require_auth()` on the `Address`
type before any state mutation. This has been stable since early SDK versions
but is worth re-verifying after any major bump.

### 5. `wasm32v1-none` build target

The workspace builds to the `wasm32v1-none` target (replacing the old
`wasm32-unknown-unknown`). This target became required with soroban-sdk
**~21.x**. Any downgrade below that would require switching the build target
back.

## Upgrading

Before bumping `soroban-sdk` in `Cargo.toml`:

1. Run `cargo test --workspace` to catch compilation and runtime breakage.
2. Run `cargo clippy --workspace --all-targets -- -D warnings` to catch
   deprecation warnings that signal upcoming removals.
3. Review the SDK [release notes / CHANGELOG](https://github.com/stellar/rs-soroban-sdk/blob/main/CHANGELOG.md)
   for breaking changes to the API shapes listed above.
4. Update the compatibility matrix in this document with the new version,
   toolchain, and any new assumptions or notes.
