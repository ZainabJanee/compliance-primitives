# Audit-Log Contract Upgrade Mechanism

## Overview

The `audit-log` contract implements Soroban's native contract upgrade mechanism via the `upgrade()` function. This lets the admin deploy bug fixes and improvements without losing recorded entries or changing the contract address that other primitives point at via `set_audit_log`.

## How It Works

### The Upgrade Function

```rust
pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error>
```

The upgrade function:
1. Requires admin authentication and checks the caller matches the stored admin (`require_admin`)
2. Emits an `UpgradePerformed` event (`admin`, `new_wasm_hash`) for auditability
3. Calls `env.deployer().update_current_contract_wasm(new_wasm_hash)` to swap the running code
4. Does **not** modify any storage — the admin key, the entry counter, and every recorded entry remain intact

### Storage Compatibility

The upgrade mechanism relies on **storage layout stability**:

- **Instance storage** (`DataKey::Admin`, `DataKey::EntryCount`) is preserved automatically by Soroban
- **Persistent storage** (`DataKey::Entry(index)`) survives the upgrade unchanged

If a new version needs to change how entries are stored (e.g., changing the `LogEntry` struct or the `DataKey` enum), a migration function must be called after the upgrade. This is a manual step, not automatic.

See [`STORAGE_VERSIONING.md`](../../STORAGE_VERSIONING.md) at the repo root for the project-wide policy on what counts as a breaking storage-layout change and what migration support is required before shipping one.

**Note on `LogEntry`:** the log is append-only and every entry is persisted under `DataKey::Entry(index)`. Adding a field to `LogEntry` is a breaking change for entries already on-chain, because old values will no longer deserialize. Do not do this without a migration step that rewrites or wraps existing entries.

## Security Model

### Who Can Upgrade?

Only the **admin address** specified during `initialize()` can trigger an upgrade. This is enforced by `require_admin()`, which:
1. Calls `admin.require_auth()` — the Soroban SDK ensures the admin has signed the transaction
2. Verifies the provided admin matches the stored admin key
3. Returns `Error::NotAuthorized` if the check fails, or `Error::NotInitialized` if `initialize` has not run

### Pause Does Not Block Upgrades

`upgrade()` is not gated on the pause flag. A paused log can still be upgraded, which is intentional: a pause is often the response to a bug, and the fix for that bug must be deployable while the log is paused.

### Storage Guarantees

1. **Instance storage is preserved**: the admin key and entry counter cannot be lost during an upgrade
2. **Persistent storage is preserved**: every recorded entry survives the upgrade as long as the storage keys remain the same
3. **Append-only ordering is preserved**: the entry counter is not reset, so new entries continue at the next index

### Auditability

Every upgrade emits an `UpgradePerformed` event with the admin's address and the new WASM hash. Auditors and on-chain observers can:
- Track all upgrades that have occurred
- Verify that only the authorized admin triggered each upgrade
- Correlate upgrades with any subsequent behavior changes

### Rollback and Safety

Soroban does **not** have automatic rollback. If a new contract version has a critical bug:
1. The admin must prepare a fixed version
2. The admin calls `upgrade()` again with the fixed wasm
3. The old (buggy) version is abandoned — only the latest deployed code runs

This is why testing the upgrade path thoroughly before mainnet deployment is critical.

## Threat Model

### If the Admin Key is Compromised

An attacker with the admin's private key can:
- Call `upgrade()` with malicious contract code, and rewrite or forge future entries
- Pause the log, blocking `record()` calls from integrating primitives
- Ship a malicious version that rewrites or hides entries in persistent storage

**Mitigation**: Keep the admin key in secure cold storage or a multisig wallet (for example, `multisig-admin`, whose own `upgrade` is threshold-gated).

### If the Upgrade Code is Buggy

If the new contract version has a bug:
- Entries may be unreadable or mis-ordered
- `record()` may silently drop or duplicate entries

**Mitigation**: Test thoroughly in testnet before mainnet. Simulate the upgrade process with a copy of the contract and existing entries.

## Upgrade Procedure

1. Build the new release WASM (`stellar contract build` / `make build`).
2. Upload it: `stellar contract upload --wasm audit_log.wasm` — this prints the new WASM hash.
3. Invoke `upgrade --admin <ADMIN> --new_wasm_hash <HASH>` on the deployed audit log, signed by the admin (or the `multisig-admin` quorum if the admin is a multisig).
4. Confirm the `UpgradePerformed` event was emitted, then call `entry_count()` and `get_entry(0)` to verify state is intact.
5. If the new version requires a storage migration, run the migration entrypoint as a separate, explicit admin call **after** `upgrade()` completes. Never run migrations implicitly inside `upgrade()`.

## Testing the Upgrade Path

### Test Scenario: Record Entries, Upgrade, Verify Survival

1. Initialize the contract
2. Record one or more entries (e.g., a `deny_add` event for Alice)
3. Call `upgrade()` with the new wasm hash
4. Read back `entry_count()` and `get_entry(index)` — they should match the pre-upgrade values
5. Call `record()` and confirm the new entry lands at the next index
6. Verify the new version handles storage the same way

### Test Scenario: Non-Admin Cannot Upgrade

1. Initialize the contract
2. Have a non-admin call `upgrade()` — should return `Error::NotAuthorized`
3. Verify the contract was not actually upgraded (the admin-only `pause` / `unpause` still behave as before)

## Comparison to Other Chains

**Ethereum/EVM**: Typically use the Proxy pattern (a separate proxy contract forwards calls to an implementation contract). Storage is in the proxy, implementation can be swapped.

**Soroban**: Uses direct contract code replacement. Storage layout must remain compatible, or a migration is needed.

## Future Enhancements

1. **Storage migration helper**: Add a dedicated `migrate_storage()` function if the entry layout ever needs to change
2. **Version tracking**: Store the contract version in storage to help with future migrations
3. **Staged upgrades**: A multi-sig admin could gate upgrades behind governance votes before calling `upgrade()`

## References

- Soroban Host Function: `update_current_contract_wasm` ([Soroban SDK docs](https://docs.rs/soroban-sdk/latest/soroban_sdk/struct.Deployer.html#method.update_current_contract_wasm))
- Storage Model: [Soroban Storage](https://soroban.stellar.org/docs/learn/storing-data)
