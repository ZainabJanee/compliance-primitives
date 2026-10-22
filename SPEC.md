# Compliance Primitives Security Specification

This document specifies the security assumptions, invariants, and threat models for each of the nine contract crates in this workspace: the original three primitives (`denylist-gate`, `allowlist-token`, `jurisdiction-flag`) plus the six added since (`audit-log`, `circuit-breaker`, `compliance-aggregator`, `multisig-admin`, `pausable`, `policy-engine`). See [`ARCHITECTURE.md`](ARCHITECTURE.md) for how these nine fit together as primitive, control, composition, and shared-library contracts.

## Executive Summary

- **Threat model**: Assumes the issuer/admin key is secure. If compromised, an attacker can manipulate all compliance decisions.
- **No recovery mechanism**: These are stateless gates; a blocked transfer cannot be undone retroactively. The issuer must maintain clear audit trails.
- **Composability**: Each primitive is independently auditable and can be called from multiple contracts. Blocking a single primitive is equivalent to blocking at the entry point.

---

## 1. Denylist-Gate Contract

**Source**: [`contracts/denylist-gate/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/lib.rs)

### 1.1 Purpose

Maintain a standalone denylist contract that other contracts call via cross-contract invocation to check if an address is clear to transact.

### 1.2 Security Assumptions

- **Admin key is secure**: The admin address is kept in secure cold storage or a multisig, never exposed.
- **Immutable contract address**: Once deployed, the contract's address does not change (users of the contract hardcode this address). If the contract needs an upgrade, the issuer must deploy a new instance and migrate callers.
- **No private state in the blockchain**: Any address on the denylist is visible on-chain. If privacy is required, this contract is not suitable.
- **Soroban host environment is secure**: The `update_current_contract_wasm` function (if used for upgrades) is not compromised.

### 1.3 Invariants

#### Invariant 1.3.1: Check Returns False for Denied Addresses
**Statement**: If address `A` is on the denylist, `check(A)` must return `false`.

**Proof**: The `check()` function at line 88-94 of `src/lib.rs`:
```rust
pub fn check(env: Env, address: Address) -> bool {
    !env.storage().persistent().get(&DataKey::Denied(address)).unwrap_or(false)
}
```
Returns `true` only if the persistent storage entry for `DataKey::Denied(address)` is absent or falsy. If `add_to_denylist` has been called with that address, the entry exists and is set to `true`, so `check()` returns `false`.

**Tests verifying this invariant**:
- `contracts/denylist-gate/src/test.rs:test_check_returns_true_for_non_denied` — calls `check()` on address that was never added, expects `true`
- `contracts/denylist-gate/src/test.rs:test_check_returns_false_for_denied` — adds to denylist, calls `check()`, expects `false`
- `examples/denylist-gate-consumer/src/test.rs:test_transfer_blocks_denylisted_from` — a token contract calls `check()` on a denied address, expects rejection

#### Invariant 1.3.2: Check Returns True for Non-Denied Addresses
**Statement**: If address `A` is not on the denylist (or has been removed), `check(A)` must return `true`.

**Proof**: Same as Invariant 1.3.1 — the negation logic ensures `true` is returned when the entry is absent.

**Tests verifying this invariant**:
- `contracts/denylist-gate/src/test.rs:test_check_returns_true_for_non_denied`
- `contracts/denylist-gate/src/test.rs:test_remove_from_denylist` — adds then removes, calls `check()`, expects `true`

#### Invariant 1.3.3: Only Admin Can Modify Denylist
**Statement**: Only the admin address (verified by `require_auth()` and storage match) can add to or remove from the denylist.

**Proof**: Both `add_to_denylist()` and `remove_from_denylist()` call `Self::require_admin()` (line 66, 76), which:
1. Calls `admin.require_auth()` (line 97) — Soroban SDK enforces the signature
2. Retrieves the stored admin (line 98) — must match the passed `admin` or returns `NotAuthorized`

**Tests verifying this invariant**:
- `contracts/denylist-gate/src/test.rs:test_add_to_denylist_fails_if_not_admin` — non-admin calls `add_to_denylist()`, expects `Error::NotAuthorized`
- `contracts/denylist-gate/src/test.rs:test_remove_from_denylist_fails_if_not_admin` — non-admin calls `remove_from_denylist()`, expects `Error::NotAuthorized`

#### Invariant 1.3.4: Initialization Is Idempotent (One-Time Only)
**Statement**: `initialize()` can only succeed once. A second call must fail with `Error::AlreadyInitialized`.

**Proof**: Line 53-55:
```rust
if env.storage().instance().has(&DataKey::Admin) {
    return Err(Error::AlreadyInitialized);
}
```
Once `initialize()` succeeds, the admin is stored, and any second call finds the key and fails.

**Tests verifying this invariant**:
- `contracts/denylist-gate/src/test.rs:test_double_initialize_fails` — calls `initialize()` twice, second fails

#### Invariant 1.3.5: Functions Fail Before Initialization
**Statement**: Any function other than `initialize()` must fail with `Error::NotInitialized` if called before `initialize()`.

**Proof**: `require_admin()` (line 98-99) retrieves the admin with `ok_or(Error::NotInitialized)`, so any admin-gated call fails if the admin key is not set.

**Tests verifying this invariant**:
- `contracts/denylist-gate/src/test.rs:test_add_to_denylist_fails_before_initialize` — calls `add_to_denylist()` before `initialize()`, expects `Error::NotInitialized`

### 1.4 Threat Model

#### Threat 1.4.1: Compromised Admin Key
**Attacker capability**: Add/remove any address from the denylist; paralyze the entire compliance check.

**Impact**: 
- Can add legitimate addresses to the denylist, blocking all their transfers
- Can remove addresses that should be blocked, allowing sanctioned transactions
- Contract continues to function but at the attacker's discretion

**Mitigation**:
- Use a multisig wallet for the admin address (e.g., 2-of-3 signers)
- Keep the key in cold storage, never on a live server
- Emit events on every denylist change; monitor for unauthorized modifications

#### Threat 1.4.2: Malicious Contract Calling Denylist
**Attacker capability**: Create a fake contract that claims to respect denylist checks but doesn't.

**Impact**: The denylist itself is not compromised, but a contract calling it could ignore the `check()` result.

**Mitigation**: 
- Denylist is trustless — the contract is stateless and does not enforce transfer logic
- Issuers must verify that contracts calling the denylist actually honor `check()` results
- This is a caller responsibility, not a denylist responsibility

#### Threat 1.4.3: Upgrade Without Audit
**Attacker capability**: If contract upgrade is enabled, deploy new code that changes denylist logic.

**Impact**: Silent behavior change, potentially allowing denied addresses to pass.

**Mitigation**:
- Require admin multi-sig to approve upgrades
- Emit upgrade event for observation
- Maintain a slow upgrade process (e.g., 2-week timelock)

### 1.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 1.3.1 | `test_check_returns_false_for_denied` | [`denylist-gate/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/test.rs) |
| 1.3.2 | `test_check_returns_true_for_non_denied` | [`denylist-gate/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/test.rs) |
| 1.3.3 | `test_add_to_denylist_fails_if_not_admin` | [`denylist-gate/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/test.rs) |
| 1.3.4 | `test_double_initialize_fails` | [`denylist-gate/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/test.rs) |
| 1.3.5 | `test_add_to_denylist_fails_before_initialize` | [`denylist-gate/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/denylist-gate/src/test.rs) |

---

## 2. Allowlist-Token Contract

**Source**: [`contracts/allowlist-token/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/lib.rs)

### 2.1 Purpose

Wrap an existing SEP-41 token and only permit `transfer` calls between two addresses that are both present on an on-chain allowlist.

### 2.2 Security Assumptions

- **Admin key is secure**: The admin manages the allowlist.
- **Underlying token contract is trusted**: The token contract at `token_address` is a legitimate SEP-41 implementation (does not steal funds, does not reverse transfers).
- **No path to call underlying token directly**: If users can call the underlying token directly, they can bypass the allowlist. The issuer must deprecate the underlying token address or require all clients to use only the allowlist-token wrapper.
- **Storage is persistent across calls**: The allowlist in persistent storage is never lost between transactions.

### 2.3 Invariants

#### Invariant 2.3.1: Transfer Requires Both Parties on Allowlist
**Statement**: If `A` or `B` is not on the allowlist, `transfer(A, B, amount)` must return `Error::Blocked` and no funds must be moved.

**Proof**: Lines 106-120 check both addresses. If either is missing, the transfer is rejected at line 113-114.

**Tests verifying this invariant**:
- `contracts/allowlist-token/src/test.rs:test_transfer_blocks_if_from_not_allowed` — from not on allowlist, transfer fails
- `contracts/allowlist-token/src/test.rs:test_transfer_blocks_if_to_not_allowed` — to not on allowlist, transfer fails

#### Invariant 2.3.2: Transfer Succeeds Only if Both Parties on Allowlist
**Statement**: If both `A` and `B` are on the allowlist, `transfer(A, B, amount)` must succeed (assuming underlying token has sufficient balance).

**Proof**: Line 113-114 checks both allowlist entries, then proceeds to call the underlying token's `transfer()` at line 121.

**Tests verifying this invariant**:
- `contracts/allowlist-token/src/test.rs:test_transfer_succeeds_if_both_allowed` — both on allowlist, transfer succeeds

#### Invariant 2.3.3: Only Admin Can Modify Allowlist
**Statement**: Only the admin address can add/remove from the allowlist.

**Proof**: Both `add_to_allowlist()` and `remove_from_allowlist()` call `Self::require_admin()`, same pattern as denylist.

**Tests verifying this invariant**:
- `contracts/allowlist-token/src/test.rs:test_add_to_allowlist_fails_if_not_admin`
- `contracts/allowlist-token/src/test.rs:test_remove_from_allowlist_fails_if_not_admin`

#### Invariant 2.3.4: Blocked Transfer Does Not Call Underlying Token
**Statement**: If the allowlist check fails, the contract must NOT call `transfer()` on the underlying token.

**Proof**: Lines 113-114 return early with `Blocked` error before any cross-contract call to the token (which happens at line 121).

**Tests verifying this invariant**:
- `contracts/allowlist-token/src/test.rs:test_transfer_blocks_if_from_not_allowed` — verifies the underlying token balance did not change (using mock)
- `contracts/allowlist-token/src/test.rs:test_transfer_blocks_if_to_not_allowed` — same

#### Invariant 2.3.5: All Allowlist Changes Emit Events
**Statement**: Every call to `add_to_allowlist()` or `remove_from_allowlist()` must emit an `AllowAdd` or `AllowRemove` event.

**Proof**: Lines 116 and 124 call `AllowAdd { address }.publish(&env)` and `AllowRemove { address }.publish(&env)` respectively.

**Tests verifying this invariant**:
- `contracts/allowlist-token/src/test.rs:test_add_to_allowlist_emits_event`
- `contracts/allowlist-token/src/test.rs:test_remove_from_allowlist_emits_event`

### 2.4 Threat Model

#### Threat 2.4.1: Compromised Admin Key
**Attacker capability**: Add/remove arbitrary addresses to/from allowlist.

**Impact**: 
- Can remove legitimate users, blocking their transfers
- Can add attacker-controlled addresses, allowing theft/fraud

**Mitigation**: Use multisig for admin, cold storage, event monitoring (same as denylist).

#### Threat 2.4.2: Malicious Underlying Token
**Attacker capability**: Deploy a fake SEP-41 token at a known address; trick allowlist-token into using it.

**Impact**: The attacker can steal all funds transferred through allowlist-token.

**Mitigation**:
- Admin must verify the token address before initializing
- Issuer should emit a trusted token address from an official channel (e.g., signed announcement)
- Cannot be fixed in the allowlist-token itself (it trusts whatever address is configured)

#### Threat 2.4.3: Users Bypass Allowlist by Calling Underlying Token Directly
**Attacker capability**: If users know the underlying token address, they can call it directly, bypassing the allowlist.

**Impact**: Allowlist is rendered useless; any token holder can transfer freely.

**Mitigation**:
- Issuer must deprecate the original token address via announcement
- Optionally: the issuer can call the underlying token's owner/admin to freeze it (if possible)
- This is a process/governance problem, not a contract problem

### 2.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 2.3.1 | `test_transfer_blocks_if_from_not_allowed` | [`allowlist-token/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/test.rs) |
| 2.3.2 | `test_transfer_succeeds_if_both_allowed` | [`allowlist-token/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/test.rs) |
| 2.3.3 | `test_add_to_allowlist_fails_if_not_admin` | [`allowlist-token/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/test.rs) |
| 2.3.4 | `test_transfer_blocks_if_from_not_allowed` | [`allowlist-token/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/test.rs) |
| 2.3.5 | `test_add_to_allowlist_emits_event` | [`allowlist-token/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/allowlist-token/src/test.rs) |

---

## 3. Jurisdiction-Flag Contract

**Source**: [`contracts/jurisdiction-flag/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/lib.rs)

### 3.1 Purpose

Attach an issuer-controlled jurisdiction code to an address; other contracts can call `is_permitted_jurisdiction()` to check if an address is in an allowed set of jurisdictions.

### 3.2 Security Assumptions

- **Issuer key is secure**: The issuer controls which jurisdictions are assigned to each address.
- **Jurisdiction codes are meaningful**: The contract does not validate jurisdiction codes (e.g., ISO 3166-1 format). The issuer must use consistent codes.
- **Storage schema is stable**: If the contract is upgraded, the storage layout must not change without a migration path.

### 3.3 Invariants

#### Invariant 3.3.1: Set Jurisdiction Stores Code
**Statement**: After calling `set_jurisdiction(issuer, A, "US")`, `get_jurisdiction(A)` must return `Some("US")`.

**Proof**: Lines 69-71 write directly to persistent storage at `DataKey::Jurisdiction(address)`, and `get_jurisdiction()` reads from the same key.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_set_and_get_jurisdiction` — sets, then gets, verifies value is stored

#### Invariant 3.3.2: Only Issuer Can Set Jurisdiction
**Statement**: Only the issuer address can call `set_jurisdiction()`.

**Proof**: Line 68 calls `Self::require_issuer()`, same pattern as denylist/allowlist.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_set_jurisdiction_rejects_non_issuer` — non-issuer calls, expects `Error::NotAuthorized`

#### Invariant 3.3.3: Is Permitted Jurisdiction Returns True Only If In Allowed List
**Statement**: `is_permitted_jurisdiction(A, allowed_codes)` returns `true` iff `A` has a jurisdiction code AND that code appears in `allowed_codes`.

**Proof**: Lines 85-88:
```rust
match Self::get_jurisdiction(env, address) {
    Some(code) => allowed_codes.iter().any(|c| c == code),
    None => false,
}
```
Returns `true` only if the address has a code AND it matches at least one in `allowed_codes`.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_is_permitted_jurisdiction_true_when_code_in_list` — code is in list, returns `true`
- `contracts/jurisdiction-flag/src/test.rs:test_is_permitted_jurisdiction_false_when_no_jurisdiction_set` — no code set, returns `false`
- `contracts/jurisdiction-flag/src/test.rs:test_is_permitted_jurisdiction_false_with_empty_allowed_list` — empty allowed list, returns `false`

#### Invariant 3.3.4: Upgrade Preserves Storage
**Statement**: After calling `upgrade(issuer, new_wasm)`, all previously set jurisdictions must remain intact.

**Proof**: The `upgrade()` function (line 98-104) calls `env.deployer().update_current_contract_wasm()` and does NOT modify any storage. Soroban's storage model preserves persistent data across code updates.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_upgrade_preserves_jurisdiction_storage` — sets jurisdiction, upgrades, verifies it's still there

#### Invariant 3.3.5: Only Issuer Can Upgrade
**Statement**: Only the issuer can call `upgrade()`.

**Proof**: Line 102 calls `Self::require_issuer()`, enforcing issuer authentication.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_upgrade_requires_issuer_auth` — non-issuer calls `upgrade()`, expects `Error::NotAuthorized`

#### Invariant 3.3.6: All Jurisdiction Changes Emit Events
**Statement**: Every call to `set_jurisdiction()` must emit a `JurisdictionSet` event.

**Proof**: Line 72 calls `JurisdictionSet { address, code }.publish(&env)`.

**Tests verifying this invariant**:
- `contracts/jurisdiction-flag/src/test.rs:test_set_jurisdiction_emits_event` (if exists; not explicitly listed but should be added)

**Gap identified**: No test explicitly verifies event emission for `set_jurisdiction`. Recommend adding `test_set_jurisdiction_emits_event`.

### 3.4 Threat Model

#### Threat 3.4.1: Compromised Issuer Key
**Attacker capability**: Assign false jurisdiction codes to addresses.

**Impact**: 
- Can assign attacker-controlled addresses to any jurisdiction, allowing them to transact
- Can assign legitimate addresses to wrong jurisdictions, blocking them

**Mitigation**: Use multisig for issuer, cold storage, event monitoring.

#### Threat 3.4.2: Issuer Sets Inconsistent Codes
**Attacker capability**: Use inconsistent jurisdiction codes (e.g., "US", "United States", "usa" for the same country).

**Impact**: Contracts checking for exact matches will inconsistently allow/block users based on which code variant was used.

**Mitigation**:
- Recommend standardized codes (ISO 3166-1 alpha-2)
- The contract does not validate; issuer must maintain discipline
- Document the approved code list in off-chain governance

#### Threat 3.4.3: Caller Trusts Wrong Contract
**Attacker capability**: Deploy a fake jurisdiction-flag contract with permissive logic.

**Impact**: Contracts calling the fake contract will allow any address.

**Mitigation**:
- Callers must verify the jurisdiction-flag address before using it
- Issuer should announce the official address via secure channel

### 3.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 3.3.1 | `test_set_and_get_jurisdiction` | [`jurisdiction-flag/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/test.rs) |
| 3.3.2 | `test_set_jurisdiction_rejects_non_issuer` | [`jurisdiction-flag/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/test.rs) |
| 3.3.3 | `test_is_permitted_jurisdiction_true_when_code_in_list` | [`jurisdiction-flag/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/test.rs) |
| 3.3.4 | `test_upgrade_preserves_jurisdiction_storage` | [`jurisdiction-flag/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/test.rs) |
| 3.3.5 | `test_upgrade_requires_issuer_auth` | [`jurisdiction-flag/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/jurisdiction-flag/src/test.rs) |
| 3.3.6 | *GAP* — no explicit test for event emission | — |

---

## 4. Audit-Log Contract

**Source**: [`contracts/audit-log/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/lib.rs)

### 4.1 Purpose

Maintain an on-chain, append-only audit trail of compliance events. Other primitives optionally record structured events (`record(source, kind, subject, detail)`) here so that a settlement or downstream contract can verify a prior compliance event exists at invocation time, without depending on an off-chain indexer.

### 4.2 Security Assumptions

- **Admin key is secure**: stored at `initialize` but not currently used to gate `record` — see Invariant 4.3.3 for what *is* enforced.
- **`source` self-attests**: any address that can produce a valid `require_auth()` may record an entry claiming to be that `source`. The log does not verify that `source` is actually one of the "real" compliance primitives; a caller that trusts entries in this log must independently trust the deployment that wired `set_audit_log` to a given log address.
- **Storage grows without bound**: entries are never deleted (append-only by design). Deployers should expect ledger storage costs to grow linearly with `record` calls and provision persistent-storage rent accordingly.
- **Opt-in only**: this contract does nothing unless another contract is explicitly configured to call it. It is not automatically wired into any primitive on deploy.

### 4.3 Invariants

#### Invariant 4.3.1: Record Requires Initialization
**Statement**: `record(...)` must fail with `Error::NotInitialized` if `initialize()` has not been called.

**Proof**: `record()` (`src/lib.rs:170-173`) checks `env.storage().instance().has(&DataKey::Admin)` before proceeding and returns early if absent.

**Tests verifying this invariant**: none explicitly (the closest coverage is `test_double_initialize_fails`, which exercises `initialize()`'s own guard, not `record()`'s pre-initialization guard). **Gap identified** — recommend adding `test_record_before_initialize_fails`.

#### Invariant 4.3.2: Entries Are Immutable and Append-Only
**Statement**: Once written, `get_entry(index)` for a given `index` always returns the same `LogEntry`; there is no update or delete path, and `entry_count()` only increases.

**Proof**: `record()` (`src/lib.rs:178-198`) always writes to a *new* index (`EntryCount` read, written to, then incremented) — it never overwrites an existing `DataKey::Entry(index)`. No function in the contract removes a `DataKey::Entry` key or decrements `EntryCount`.

**Tests verifying this invariant**:
- `contracts/audit-log/src/test.rs:test_record_and_read_back` — records an entry, reads it back, verifies fields match
- `contracts/audit-log/src/test.rs:test_entry_count` — records multiple entries, verifies the counter increments monotonically

#### Invariant 4.3.3: Only the Claimed Source Can Record On Its Own Behalf
**Statement**: `record(source, ...)` must fail auth if the caller cannot produce a valid `require_auth()` for `source`.

**Proof**: `src/lib.rs:176` calls `source.require_auth()` before any storage write — Soroban's host enforces this against the invocation's signatures.

**Tests verifying this invariant**:
- `contracts/audit-log/src/test.rs:test_unauthorized_record_rejected` — calls `record()` without authorizing as `source`, expects rejection

#### Invariant 4.3.4: Out-of-Range Reads Return `None`, Not an Error or Panic
**Statement**: `get_entry(index)` for an `index >= entry_count()` (or before any entry has been written) returns `None`.

**Proof**: `get_entry()` (`src/lib.rs:213-217`) uses `.get(...)` (returning `Option<LogEntry>`), not `.get(...).unwrap()` — a missing persistent key maps to `None` rather than panicking.

**Tests verifying this invariant**:
- `contracts/audit-log/src/test.rs:test_get_entry_out_of_range_returns_none`

#### Invariant 4.3.5: Every Successful Record Emits a `ComplianceEvent`
**Statement**: Every call to `record()` that returns `Ok(())` must emit a `ComplianceEvent` with matching `kind`, `subject`, `source`, and `detail`.

**Proof**: `src/lib.rs:200-206` publishes `ComplianceEvent { kind, subject, source, detail }` unconditionally on the success path, after the storage write.

**Tests verifying this invariant**:
- `contracts/audit-log/src/test.rs:test_record_emits_event`

#### Invariant 4.3.6: Initialization Is Idempotent (One-Time Only)
**Statement**: `initialize()` can only succeed once; a second call fails with `Error::AlreadyInitialized`.

**Proof**: `src/lib.rs:145-147`, same pattern as the three original primitives (Invariant 1.3.4).

**Tests verifying this invariant**:
- `contracts/audit-log/src/test.rs:test_double_initialize_fails`

### 4.4 Threat Model

#### Threat 4.4.1: Forged Entries from an Untrusted Source
**Attacker capability**: Any address that can authorize its own transaction can call `record()` claiming to be that address as `source` — including an address that is *not* actually one of the deployed compliance primitives.

**Impact**: A downstream contract or off-chain tool that trusts "an entry from address X exists" without verifying X is the primitive it expects could be misled by a spoofed source.

**Mitigation**: Callers that treat log entries as proof of a compliance decision must hardcode/verify the expected `source` address(es) themselves — the log makes no claim about who `source` "should" be, only that the entry's `source` field matches who actually authorized the call.

#### Threat 4.4.2: Unbounded Storage Growth
**Attacker capability**: Any address wired as a valid `source` can call `record()` repeatedly, growing the log indefinitely.

**Impact**: Rising persistent-storage rent for the deployer; no on-chain mechanism to prune old entries.

**Mitigation**: This is a known, documented tradeoff (see the module-level doc comment): use this contract for lower-frequency compliance events, not high-throughput transfer events, and monitor rent costs. A future version could add an admin-gated archival/pruning path if this becomes an issue in practice.

#### Threat 4.4.3: Admin Key Provides No Access Control Today
**Attacker capability**: N/A directly — `admin` is stored but not currently checked by any function.

**Impact**: A reader might assume `admin` gates who can `record()`; it does not. This is a documentation/expectation risk rather than an exploitable one today, but is worth flagging for auditors: if a future change adds admin-gating to `record()`, existing integrations that call `record()` as a non-admin `source` would break.

**Mitigation**: The module doc comment already states this explicitly ("not currently used for access control on `record`; it is stored for future governance operations"). No code change needed; auditors should confirm this stays documented if the access-control model changes.

### 4.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 4.3.1 | *GAP* — no explicit test for `record()` before `initialize()` | — |
| 4.3.2 | `test_record_and_read_back`, `test_entry_count` | [`audit-log/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/test.rs) |
| 4.3.3 | `test_unauthorized_record_rejected` | [`audit-log/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/test.rs) |
| 4.3.4 | `test_get_entry_out_of_range_returns_none` | [`audit-log/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/test.rs) |
| 4.3.5 | `test_record_emits_event` | [`audit-log/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/test.rs) |
| 4.3.6 | `test_double_initialize_fails` | [`audit-log/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/audit-log/src/test.rs) |

---

## 5. Circuit-Breaker Contract

**Source**: [`contracts/circuit-breaker/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/circuit-breaker/src/lib.rs)

### 5.1 Purpose

Provide a single, shared, admin-controlled emergency stop (`freeze`/`unfreeze`/`is_frozen`) that a consumer contract checks *before* any compliance primitive, so one transaction can halt every composed check during an incident rather than pausing each primitive independently. See [`docs/emergency-freeze-design.md`](docs/emergency-freeze-design.md).

### 5.2 Security Assumptions

- **Admin key is secure**: single-admin model by design (see Threat 5.4.1) — the smallest, most operationally simple option, with the tradeoff documented as an explicit risk to weigh at deployment time (e.g. pairing with `multisig-admin` as the admin address).
- **Consumers actually check it**: like the original primitives' `check()` functions, `is_frozen()` is advisory — the circuit breaker cannot force a consumer contract to call it. A consumer that omits the check gets no protection.
- **Freeze is global, not per-primitive**: one `circuit-breaker` instance is meant to gate an entire deployment's compliance path, not a single contract.

### 5.3 Invariants

#### Invariant 5.3.1: Freeze Defaults to Unfrozen
**Statement**: Immediately after `initialize()`, `is_frozen()` returns `false`.

**Proof**: `initialize()` (`src/lib.rs:26-34`) explicitly sets `DataKey::Frozen` to `false`.

**Tests verifying this invariant**:
- `contracts/circuit-breaker/src/test.rs:test_is_frozen_defaults_to_false`

#### Invariant 5.3.2: Only Admin Can Freeze or Unfreeze
**Statement**: `freeze()`/`unfreeze()` must fail with `Error::NotAuthorized` (or `Error::NotInitialized` if uninitialized) for any caller other than the stored admin.

**Proof**: Both call `Self::require_admin()` (`src/lib.rs:52-63`), which requires `admin.require_auth()` and compares against the stored admin, identical in shape to Invariant 1.3.3.

**Tests verifying this invariant**:
- `contracts/circuit-breaker/src/test.rs:test_non_admin_cannot_freeze_or_unfreeze`

#### Invariant 5.3.3: Freeze/Unfreeze Round-Trip Correctly
**Statement**: After `freeze()`, `is_frozen()` returns `true`; after a subsequent `unfreeze()`, it returns `false` again.

**Proof**: `freeze()`/`unfreeze()` (`src/lib.rs:36-46`) write `true`/`false` directly to `DataKey::Frozen`; `is_frozen()` (`src/lib.rs:48-50`) reads the same key.

**Tests verifying this invariant**:
- `contracts/circuit-breaker/src/test.rs:test_admin_can_freeze_and_unfreeze`

#### Invariant 5.3.4: Initialization Is Idempotent
**Statement**: `initialize()` can only succeed once.

**Proof**: `src/lib.rs:27-29`, identical pattern to Invariant 1.3.4. **Gap identified** — no dedicated `test_double_initialize_fails` exists for this contract; recommend adding one for parity with the other eight contracts.

**Tests verifying this invariant**: none explicit — covered only implicitly by the fact that `setup()` in the test module calls `initialize()` exactly once per test.

### 5.4 Threat Model

#### Threat 5.4.1: Compromised or Unavailable Admin Key
**Attacker capability**: Freeze/unfreeze at will if the key is compromised; conversely, if the admin key is lost, the breaker can never be unfrozen (or frozen) again.

**Impact**: A compromised key can DoS every consumer that checks `is_frozen()` by freezing indefinitely; a lost key removes the emergency-stop capability entirely, or freezes the system permanently if lost while frozen.

**Mitigation**: `docs/emergency-freeze-design.md` explicitly recommends a multisig or threshold key (e.g. deploying `multisig-admin` and passing its address as `circuit-breaker`'s `admin`) for production use rather than a single EOA.

#### Threat 5.4.2: Consumer Fails to Check the Breaker
**Attacker capability**: N/A — this is an integration risk, not an attack on the contract itself.

**Impact**: A consumer contract that never calls `is_frozen()` receives no protection from a freeze; an admin who believes freezing halts *all* activity would be wrong for that consumer.

**Mitigation**: Document the requirement clearly for integrators (as this contract's design doc does) and provide a reference implementation (`examples/`) that shows the check being made first in the transfer path.

#### Threat 5.4.3: Freeze/Unfreeze Race with In-Flight Transactions
**Attacker capability**: Submit a transaction just before a freeze lands.

**Impact**: Soroban ledger close ordering determines whether a given transaction executes before or after a `freeze()` call lands in the same or an earlier ledger; there is no guarantee a "just in time" transaction is blocked.

**Mitigation**: This is an inherent limitation of any on-chain switch, not specific to this contract. Operationally, treat `freeze()` as a mitigation for *ongoing* incidents, not a way to guarantee zero further transactions land within the same ledger close.

### 5.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 5.3.1 | `test_is_frozen_defaults_to_false` | [`circuit-breaker/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/circuit-breaker/src/test.rs) |
| 5.3.2 | `test_non_admin_cannot_freeze_or_unfreeze` | [`circuit-breaker/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/circuit-breaker/src/test.rs) |
| 5.3.3 | `test_admin_can_freeze_and_unfreeze` | [`circuit-breaker/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/circuit-breaker/src/test.rs) |
| 5.3.4 | *GAP* — no explicit double-initialize test | — |

---

## 6. Compliance-Aggregator Contract

**Source**: [`contracts/compliance-aggregator/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/lib.rs)

### 6.1 Purpose

Reduce a consumer's cross-contract call overhead by batching a `denylist-gate` check and a `jurisdiction-flag` check into one call (`check_address`/`check_all`), AND-composing the results with a per-check breakdown for auditability. See [`ARCHITECTURE.md`](ARCHITECTURE.md#composition-contracts) for how this relates to `policy-engine`.

### 6.2 Security Assumptions

- **Admin key is secure**: governs which `denylist-gate`/`jurisdiction-flag` addresses are trusted.
- **Registered addresses are the real primitives**: this contract does not verify that the registered `DenylistGate`/`JurisdictionFlag` addresses are genuine deployments of those contracts — a malicious or misconfigured address registered by a compromised/careless admin would have its (possibly permissive) response trusted as-is.
- **Checks are AND-only**: there is no OR mode here (see `policy-engine` for that). A consumer that needs OR semantics must use a different contract.
- **Both checks are optional but at least one must be registered**: calling `check_address`/`check_all` with zero registered checks is treated as a misconfiguration, not a vacuous pass.

### 6.3 Invariants

#### Invariant 6.3.1: `check_address` AND-Composes All Registered Checks
**Statement**: `check_address(address, codes)` returns `all_passed = true` iff every registered check (denylist, jurisdiction, or both — whichever are configured) returns `true` for `address`.

**Proof**: `src/lib.rs:273-316` initializes `all_passed = true` and only ever narrows it with `all_passed && passed` per registered check (`src/lib.rs:289`, `304`); it is never widened back to `true`.

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_both_checks_pass`
- `contracts/compliance-aggregator/src/test.rs:test_denylist_fail_jurisdiction_pass`
- `contracts/compliance-aggregator/src/test.rs:test_denylist_pass_jurisdiction_fail`
- `contracts/compliance-aggregator/src/test.rs:test_both_checks_fail`

#### Invariant 6.3.2: Unregistered Check Types Are Skipped, Not Failed
**Statement**: If only one of `DenylistGate`/`JurisdictionFlag` is registered, `check_address` evaluates only that one check and does not treat the unregistered check type as a failure.

**Proof**: Each check block (`src/lib.rs:282-294`, `297-309`) is gated by `if let Some(addr) = ...`; when the address is absent, the block is skipped entirely rather than contributing a `false` result.

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_check_address_denylist_only_pass`
- `contracts/compliance-aggregator/src/test.rs:test_check_address_jurisdiction_only_pass`

#### Invariant 6.3.3: Zero Registered Checks Is Rejected, Not Vacuously Passed
**Statement**: `check_address`/`check_all` return `Error::NoChecksRegistered` if neither `DenylistGate` nor `JurisdictionFlag` is registered, rather than returning `all_passed = true` with an empty result list.

**Proof**: `check_address` (`src/lib.rs:311-313`) checks `results.is_empty()` after both optional blocks and errors before returning. `check_all` (`src/lib.rs:351-353`) checks both addresses are `None` up front.

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_check_address_no_checks_registered`
- `contracts/compliance-aggregator/src/test.rs:test_check_all_no_checks_registered`

#### Invariant 6.3.4: `check_all` Rejects an Empty Address List
**Statement**: `check_all(addresses, codes)` returns `Error::EmptyAddressList` if `addresses` is empty, regardless of what's registered.

**Proof**: `src/lib.rs:336-338`, checked before any registered-check lookup.

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_check_all_empty_list_error`

#### Invariant 6.3.5: Only Admin Can Register or Replace Check Addresses
**Statement**: `set_denylist_gate`, `set_jurisdiction_flag`, and `set_admin` all fail for a non-admin caller.

**Proof**: All three call `Self::require_admin()` (`src/lib.rs:396-407`) before any storage write, same pattern as Invariant 1.3.3.

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_set_admin_rejects_non_admin`
- `contracts/compliance-aggregator/src/test.rs:test_set_denylist_gate_rejects_non_admin`
- `contracts/compliance-aggregator/src/test.rs:test_set_jurisdiction_flag_rejects_non_admin`

#### Invariant 6.3.6: `check_all` Preserves Per-Address Order and Per-Check Order
**Statement**: The `Vec<AddressCheckResult>` returned by `check_all` is in the same order as the input `addresses`, and within each result, `checks` lists denylist before jurisdiction (when both are registered).

**Proof**: `src/lib.rs:357-387` iterates `addresses.iter()` and pushes to `batch` in order; within each iteration, the denylist block (`361-369`) always runs before the jurisdiction block (`371-380`).

**Tests verifying this invariant**:
- `contracts/compliance-aggregator/src/test.rs:test_check_all_mixed_results`

### 6.4 Threat Model

#### Threat 6.4.1: Compromised Admin Registers a Malicious Check Contract
**Attacker capability**: Replace the registered `denylist-gate`/`jurisdiction-flag` address with an attacker-controlled contract that always returns `true`.

**Impact**: Every downstream consumer that trusts this aggregator's `all_passed` result would accept transfers that should have been blocked — a silent compliance bypass.

**Mitigation**: Same as Threat 2.4.1/1.4.1 — multisig admin, cold storage, event monitoring on `DenylistGateSet`/`JurisdictionFlagSet`.

#### Threat 6.4.2: Cross-Contract Call to a Non-Responsive or Malformed Contract
**Attacker capability**: N/A directly, but an admin misconfiguration (registering a non-contract address, or a contract that doesn't implement the expected interface) is a realistic operational failure mode.

**Impact**: `check_address`/`check_all` would panic or trap rather than returning a typed error, since the generated client's call has no fallback path for a malformed callee.

**Mitigation**: Admin should verify a newly-registered address responds correctly (e.g. via a testnet dry run) before pointing production traffic at it. Not something the contract itself can fully guard against, given Soroban's cross-contract call model.

#### Threat 6.4.3: Consumer Misreads `checks` for Registration State
**Attacker capability**: N/A — an integration risk.

**Impact**: A consumer that assumes `checks.len() == 2` always holds (both checks registered) would misinterpret results after only one check type is registered (Invariant 6.3.2) — e.g. treating `all_passed = true` from a single-check aggregator as "cleared both denylist and jurisdiction" when only one was actually configured.

**Mitigation**: Consumers should inspect `CheckResult.check` (the `CheckKind`) in the returned list rather than assuming a fixed length, or should query `denylist_gate()`/`jurisdiction_flag()` to confirm what's registered before trusting the aggregate.

### 6.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 6.3.1 | `test_both_checks_pass`, `test_denylist_fail_jurisdiction_pass`, `test_denylist_pass_jurisdiction_fail`, `test_both_checks_fail` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |
| 6.3.2 | `test_check_address_denylist_only_pass`, `test_check_address_jurisdiction_only_pass` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |
| 6.3.3 | `test_check_address_no_checks_registered`, `test_check_all_no_checks_registered` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |
| 6.3.4 | `test_check_all_empty_list_error` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |
| 6.3.5 | `test_set_admin_rejects_non_admin`, `test_set_denylist_gate_rejects_non_admin`, `test_set_jurisdiction_flag_rejects_non_admin` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |
| 6.3.6 | `test_check_all_mixed_results` | [`compliance-aggregator/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/compliance-aggregator/src/test.rs) |

---

## 7. Multisig-Admin Contract

**Source**: [`contracts/multisig-admin/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/lib.rs)

### 7.1 Purpose

Implement M-of-N multisig authorization via Soroban's `CustomAccountInterface`, so its contract address can be dropped in as the `admin`/`issuer` of any primitive without modifying that primitive. See [`DESIGN_MULTISIG_ADMIN.md`](DESIGN_MULTISIG_ADMIN.md) for the full design rationale and the tradeoff table versus a built-in per-primitive multisig.

### 7.2 Security Assumptions

- **`__check_auth` is only ever invoked by the Soroban host**, in response to a `require_auth()` against this contract's address — it is not a normal callable entry point a user invokes directly with arbitrary arguments.
- **Each signer independently authorizes**: the multisig model relies on Soroban's auth framework requiring every address listed in `signatures` to itself have a valid signature in the transaction; this contract does not (and cannot) fabricate a signer's authorization.
- **Signer-set mutations are self-gated**: `add_signer`/`remove_signer`/`update_threshold` require the *current* threshold to be met (via re-entering `__check_auth`), not a separate, weaker admin key — there is no single-key backdoor to the signer set.
- **`threshold` is always within `[1, signers.len()]`**: enforced at every mutation point (see Invariant 7.3.2 and 7.3.6).

### 7.3 Invariants

#### Invariant 7.3.1: Authorization Succeeds Only at or Above Threshold
**Statement**: `__check_auth(payload, signatures, context)` returns `Ok(())` iff the number of addresses in `signatures` that are both (a) present in the stored signer set and (b) individually authorized via their own `require_auth()`, is `>= threshold`.

**Proof**: `src/lib.rs:250-288` counts `valid_count` by checking membership (`274-280`) only after calling `sig_addr.require_auth()` (`272`) for each candidate signature, then compares against `threshold` (`283-287`).

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_multisig_as_denylist_admin_with_mock_auth` — end-to-end: multisig set as a primitive's admin, mutation succeeds only with enough approving signers
- `contracts/multisig-admin/src/test.rs:test_threshold_not_met_error_value` — below-threshold signatures produce `Error::ThresholdNotMet`

#### Invariant 7.3.2: Threshold Is Always Valid Relative to Signer Count
**Statement**: At no point can stored state have `threshold == 0` or `threshold > signers.len()`.

**Proof**: `initialize()` (`src/lib.rs:114-116`) rejects `threshold == 0` or `threshold > signers.len()`; `update_threshold()` (`src/lib.rs:199-201`) re-checks the same bounds against the *current* signer count; `remove_signer()` (`src/lib.rs:181-183`) rejects a removal that would drop `signers.len()` below the stored `threshold`.

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_invalid_threshold_zero_rejected`
- `contracts/multisig-admin/src/test.rs:test_invalid_threshold_exceeds_signer_count`
- `contracts/multisig-admin/src/test.rs:test_remove_signer_rejected_when_count_drops_below_threshold`
- `contracts/multisig-admin/src/test.rs:test_update_threshold_invalid_rejected`

#### Invariant 7.3.3: Signer-Set Mutations Require the Multisig Itself
**Statement**: `add_signer`, `remove_signer`, and `update_threshold` each require `env.current_contract_address().require_auth()` — i.e. they re-enter `__check_auth` and are subject to the same threshold as any other admin operation gated by this contract.

**Proof**: `src/lib.rs:132`, `155`, `191` — each calls `env.current_contract_address().require_auth()` as its first line.

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_signer_update_requires_multisig_auth`

#### Invariant 7.3.4: No Duplicate Signers
**Statement**: `add_signer(new_signer)` fails with `Error::AlreadySigner` if `new_signer` is already in the signer set.

**Proof**: `src/lib.rs:141-145` scans the existing set and returns early on a match, before `push_back`.

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_add_duplicate_signer_rejected`

#### Invariant 7.3.5: Removing a Non-Signer Fails
**Statement**: `remove_signer(signer)` fails with `Error::SignerNotFound` if `signer` is not currently in the set.

**Proof**: `src/lib.rs:169-176` sets `found_index = None` by default and only proceeds past the `.ok_or(Error::SignerNotFound)?` if a match was found.

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_remove_signer_not_found_rejected`

#### Invariant 7.3.6: Initialization Is Idempotent
**Statement**: `initialize()` can only succeed once.

**Proof**: `src/lib.rs:111-113`, keyed on `DataKey::Threshold` rather than `DataKey::Admin` (this contract has no single `Admin` key — the signer set itself is the authority).

**Tests verifying this invariant**:
- `contracts/multisig-admin/src/test.rs:test_double_initialize_fails`

### 7.4 Threat Model

#### Threat 7.4.1: Signer Collusion Below Threshold Intent
**Attacker capability**: Any `threshold` signers (out of N) can authorize any admin operation on every primitive that has this contract set as its admin — by design.

**Impact**: If `threshold` is set too low relative to N (e.g. 1-of-5), the "multisig" provides little more protection than a single key, since compromising or colluding with any one signer suffices.

**Mitigation**: Choose `threshold` deliberately (e.g. majority or supermajority of N) at `initialize` time; `update_threshold` itself requires meeting the *current* threshold, so a signer set cannot unilaterally weaken its own policy without sufficient existing approval.

#### Threat 7.4.2: Signer-Set Growth/Shrinkage Games the Threshold
**Attacker capability**: Signers meeting the current threshold can add new signers (diluting existing signers' relative weight) or remove signers (concentrating it), as long as `threshold <= signers.len()` continues to hold.

**Impact**: Over time, without governance discipline, the signer set composition and effective security margin can drift from what was originally intended, without ever technically violating Invariant 7.3.2.

**Mitigation**: This is a governance-process concern, not a contract bug — the contract enforces its stated invariants correctly. Deployers should pair this contract with off-chain governance policy (e.g. requiring public announcement before any signer-set change) and monitor for `add_signer`/`remove_signer` calls.

#### Threat 7.4.3: `__check_auth` Reuse Across Unrelated Contexts
**Attacker capability**: N/A directly — `auth_context` and `signature_payload` are accepted but not inspected by this reference implementation (`#[allow(unused_variables)]`, `src/lib.rs:249`).

**Impact**: A future extension that wants per-context policy (e.g. a lower threshold for read-only operations, a higher one for signer-set changes) is not implemented here; today, every operation gated by this contract's address uses the same flat threshold regardless of what's being authorized.

**Mitigation**: Documented limitation, not a vulnerability in the current scope — the module doc comment already scopes this as a "reference implementation." Auditors of a deployment relying on context-sensitive policy should confirm this contract has been extended accordingly before relying on it for that purpose.

### 7.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 7.3.1 | `test_multisig_as_denylist_admin_with_mock_auth`, `test_threshold_not_met_error_value` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |
| 7.3.2 | `test_invalid_threshold_zero_rejected`, `test_invalid_threshold_exceeds_signer_count`, `test_remove_signer_rejected_when_count_drops_below_threshold`, `test_update_threshold_invalid_rejected` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |
| 7.3.3 | `test_signer_update_requires_multisig_auth` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |
| 7.3.4 | `test_add_duplicate_signer_rejected` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |
| 7.3.5 | `test_remove_signer_not_found_rejected` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |
| 7.3.6 | `test_double_initialize_fails` | [`multisig-admin/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/multisig-admin/src/test.rs) |

---

## 8. Pausable (Shared Library)

**Source**: [`contracts/pausable/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/pausable/src/lib.rs)

### 8.1 Purpose

Provide identical pause/unpause/`require_not_paused` logic to `allowlist-token`, `denylist-gate`, and `jurisdiction-flag` from a single audited crate, compiled into each contract rather than deployed or called separately. See [`docs/pausable-design.md`](docs/pausable-design.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md#shared-library-pausable) for why this is a library rather than a fourth deployable contract.

### 8.2 Security Assumptions

- **No independent access control**: this crate has no admin concept of its own — `is_paused`/`pause`/`unpause` are unguarded free functions. Every guarantee about *who* may call `pause`/`unpause` comes entirely from the depending contract wrapping these calls in its own admin/issuer check before invoking them (see Invariant 8.3.4).
- **Storage key is fixed and shared with the depending contract's instance storage**: the `"Paused"` symbol key must not collide with another key the depending contract uses in its own `DataKey` enum. This is a per-contract integration responsibility, not something this crate can enforce.
- **No wasm exports**: since this crate compiles into its caller, there is no independent contract to attack directly — its logic can only be reached through whichever contract embeds it.

### 8.3 Invariants

#### Invariant 8.3.1: Defaults to Unpaused
**Statement**: `is_paused(env)` returns `false` when the `"Paused"` key has never been set.

**Proof**: `is_paused()` (`src/lib.rs:53-58`) uses `.unwrap_or(false)` on a missing key.

**Tests verifying this invariant**:
- `contracts/pausable/src/lib.rs:test_not_paused_by_default`

#### Invariant 8.3.2: Pause/Unpause Round-Trip Correctly
**Statement**: After `pause(env)`, `is_paused(env)` returns `true`; after a subsequent `unpause(env)`, it returns `false`.

**Proof**: `pause()` (`src/lib.rs:64-68`) sets `"Paused"` to `true`; `unpause()` (`src/lib.rs:73-77`) *removes* the key entirely (rather than setting it to `false`), which `is_paused`'s `unwrap_or(false)` treats identically to never having been set.

**Tests verifying this invariant**:
- `contracts/pausable/src/lib.rs:test_pause_and_unpause`

#### Invariant 8.3.3: `pause`/`unpause` Are Idempotent
**Statement**: Calling `pause()` (or `unpause()`) twice in a row is equivalent to calling it once — no error, no state beyond simple boolean toggling.

**Proof**: Both functions unconditionally `set`/`remove` the key with no precondition check (`src/lib.rs:64-68`, `73-77`); calling either twice leaves storage in the same state as calling it once.

**Tests verifying this invariant**:
- `contracts/pausable/src/lib.rs:test_pause_is_idempotent`

#### Invariant 8.3.4: `require_not_paused` Fails Exactly When Paused
**Statement**: `require_not_paused(env, err)` returns `Ok(())` iff `is_paused(env)` is `false`, and `Err(err)` (the caller-supplied error value) otherwise.

**Proof**: `src/lib.rs:89-95` is a direct `if is_paused(env) { Err(paused_err) } else { Ok(()) }`.

**Tests verifying this invariant**:
- `contracts/pausable/src/lib.rs:test_require_not_paused_ok_when_unpaused`
- `contracts/pausable/src/lib.rs:test_require_not_paused_err_when_paused`

### 8.4 Threat Model

#### Threat 8.4.1: Depending Contract Forgets to Gate `pause`/`unpause`
**Attacker capability**: Since this crate exposes `pause`/`unpause` as unguarded free functions, any depending contract that calls them without its own admin check would let *any* caller pause the contract.

**Impact**: A trivial denial-of-service on that specific contract's mutating operations.

**Mitigation**: This is squarely an integration responsibility, called out in the crate's own doc comment ("Adds `pause`, `unpause`, and `is_paused` methods gated by the contract's existing admin/issuer auth helper"). Reviewers of any new contract adopting this crate should confirm the admin check is present before the call into `pause`/`unpause`, exactly as `allowlist-token` and `jurisdiction-flag` already do.

#### Threat 8.4.2: Depending Contract Forgets to Call `require_not_paused`
**Attacker capability**: N/A directly — an integration gap, not an attack on this crate.

**Impact**: A state-mutating method that omits the `require_not_paused` check at its top would continue operating while the contract is nominally "paused," silently defeating the safety mechanism for that one method.

**Mitigation**: Code review checklist item for any change to a contract that depends on this crate: every mutating method must call `require_not_paused` first (per the crate's documented usage pattern); read-only methods are deliberately exempt.

#### Threat 8.4.3: Storage Key Collision with the Depending Contract
**Attacker capability**: N/A — a coding-error risk, not something an external attacker triggers directly.

**Impact**: If a depending contract independently defines a `DataKey`/storage entry that maps to the same on-chain symbol as `"Paused"`, a write from one code path could silently corrupt the other's state.

**Mitigation**: `symbol_short!("Paused")` is a short (≤9 character) symbol chosen to be unlikely to collide with existing `DataKey` variants across the three primitives; a change to any depending contract's storage schema should grep for `"Paused"` (or use the workspace-wide `cargo test` suite, which would surface a corrupted pause state as a test failure) before introducing a new key.

### 8.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 8.3.1 | `test_not_paused_by_default` | [`pausable/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/pausable/src/lib.rs) |
| 8.3.2 | `test_pause_and_unpause` | [`pausable/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/pausable/src/lib.rs) |
| 8.3.3 | `test_pause_is_idempotent` | [`pausable/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/pausable/src/lib.rs) |
| 8.3.4 | `test_require_not_paused_ok_when_unpaused`, `test_require_not_paused_err_when_paused` | [`pausable/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/pausable/src/lib.rs) |

---

## 9. Policy-Engine Contract

**Source**: [`contracts/policy-engine/src/lib.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/policy-engine/src/lib.rs)

### 9.1 Purpose

Compose an admin-managed, mutable list of compliance checks against arbitrary primitive contracts, combined with a configurable `CombineOp` (`All` = AND, `Any` = OR), evaluated per-transfer via `evaluate(from, to)`. See [`ARCHITECTURE.md`](ARCHITECTURE.md#composition-contracts) for how this differs from `compliance-aggregator`.

### 9.2 Security Assumptions

- **Admin key is secure**: governs the check list and, indirectly, which contract addresses are trusted for `evaluate`.
- **Registered check contracts are the real primitives**: same caveat as Invariant 6.2 for `compliance-aggregator` — `CheckKind::Denylist { contract }`/`CheckKind::Jurisdiction { contract, .. }` store admin-supplied addresses with no on-chain verification that they are genuine deployments.
- **`evaluate` never rolls back on a failed policy**: by design, a failing policy returns `Ok(false)` with an emitted `PolicyResult` event rather than a contract error, specifically so the audit trail (the event) survives — a Soroban error return rolls back all state changes *including emitted events*. Any caller that expects a failed policy to also revert its *own* state change must check the returned `bool` itself and revert explicitly.
- **`CombineOp::Any` with zero checks is `false`, not `true`**: an empty check list under OR semantics is explicitly defined as failing (see Invariant 9.3.3), avoiding the vacuous-truth trap where "no configured check failed" would otherwise read as "passed."

### 9.3 Invariants

#### Invariant 9.3.1: `CombineOp::All` Requires Every Check to Pass for Both Addresses
**Statement**: With `op = All`, `evaluate(from, to)` returns `Ok(true)` iff every registered check passes for **both** `from` and `to`.

**Proof**: `src/lib.rs:222-234` iterates all checks, short-circuiting to `all_pass = false` and breaking on the first check where `run_check(..., from)` or `run_check(..., to)` is `false`; if the loop completes, `all_pass` remains `true`.

**Tests verifying this invariant**:
- `contracts/policy-engine/src/test.rs:test_all_checks_pass`
- `contracts/policy-engine/src/test.rs:test_one_check_fails_and_semantics`

#### Invariant 9.3.2: `CombineOp::Any` Requires At Least One Check to Pass for Both Addresses
**Statement**: With `op = Any`, `evaluate(from, to)` returns `Ok(true)` iff at least one registered check passes for **both** `from` and `to` (a check passing for only one of the two does not count).

**Proof**: `src/lib.rs:236-253` requires `run_check(..., from) && run_check(..., to)` together (`244-246`) before setting `any_pass = true` for that check — a check is only "passing" for this purpose if it clears both addresses.

**Tests verifying this invariant**:
- `contracts/policy-engine/src/test.rs:test_one_check_passes_or_semantics`

#### Invariant 9.3.3: `CombineOp::Any` with No Registered Checks Fails Closed
**Statement**: With `op = Any` and an empty check list, `evaluate` returns `Ok(false)`, not `Ok(true)`.

**Proof**: `src/lib.rs:238-240` explicitly special-cases `checks.is_empty()` to `false` before entering the loop, rather than letting the loop's initial `any_pass = false` (`241`) be reached only after iterating zero elements — the explicit branch makes the fail-closed behavior a deliberate choice rather than an accident of loop semantics. (Note: `CombineOp::All` with an empty list is `Ok(true)` — vacuously all of zero checks pass — which is the standard, intended meaning of an unconfigured AND-policy; see the `CombineOp::All` arm, `222-234`, which has no equivalent empty-list special case.)

**Tests verifying this invariant**: none explicit for the empty-list `Any` case specifically. **Gap identified** — recommend adding `test_any_with_no_checks_fails_closed`.

#### Invariant 9.3.4: Every `evaluate` Call Emits a `PolicyResult` Event Regardless of Outcome
**Statement**: Both a passing and a failing `evaluate` call emit a `PolicyResult { passed, from, to }` event.

**Proof**: `src/lib.rs:256-261` publishes `PolicyResult` unconditionally, after the `match` on `op` has already determined `passed`, and before the function returns `Ok(passed)` — there is no early-return path that skips it.

**Tests verifying this invariant**: covered indirectly by `test_all_checks_pass`/`test_one_check_fails_and_semantics`/`test_one_check_passes_or_semantics`, none of which assert on `env.events()` directly. **Gap identified** — recommend adding an explicit `test_evaluate_emits_policy_result_event` (mirroring `audit-log`'s `test_record_emits_event`) for both the passing and failing case.

#### Invariant 9.3.5: Only Admin Can Mutate the Check List
**Statement**: `add_check`/`remove_check` fail for a non-admin caller.

**Proof**: Both call `Self::require_admin()` (`src/lib.rs:306-317`), same pattern as Invariant 1.3.3.

**Tests verifying this invariant**: covered indirectly — `test_add_and_remove_check` exercises the admin-authorized path via `mock_all_auths()`. **Gap identified** — no explicit `test_add_check_rejects_non_admin`/`test_remove_check_rejects_non_admin`, unlike the equivalent tests present for `compliance-aggregator` and the three original primitives.

#### Invariant 9.3.6: Initialization Is Idempotent
**Statement**: `initialize()` can only succeed once.

**Proof**: `src/lib.rs:155-157`, identical pattern to Invariant 1.3.4.

**Tests verifying this invariant**: covered only implicitly (no dedicated `test_double_initialize_fails` exists for this contract, unlike `audit-log`, `multisig-admin`, and `compliance-aggregator`). **Gap identified**.

### 9.4 Threat Model

#### Threat 9.4.1: Compromised Admin Registers a Malicious or Overly-Permissive Check
**Attacker capability**: Add a `CheckKind` pointing at an attacker-controlled contract, or remove legitimate checks, or flip `CombineOp` between `All`/`Any` — though `CombineOp` is fixed at `initialize` and this contract exposes no method to change it after the fact, so this last item is not actually reachable post-initialization.

**Impact**: Same class of impact as Threat 6.4.1 — a compromised admin can make `evaluate` return `true` for addresses that should fail, or make legitimate addresses fail via `All` semantics with a spurious extra check.

**Mitigation**: Same as the other admin-key threats in this document — multisig admin, cold storage, event monitoring (`PolicyResult` events give a full audit trail of every evaluation outcome, which aids detecting an admin-driven policy change with unusual results).

#### Threat 9.4.2: `Any` Semantics Weaken the Effective Policy Below Operator Intent
**Attacker capability**: N/A directly — a configuration-design risk rather than an attack.

**Impact**: An admin who intends "these checks are all independently important" but configures `CombineOp::Any` would find that satisfying just one check (e.g. an easier KYC provider) is sufficient to pass, potentially undermining the intent behind the stricter check(s) also registered.

**Mitigation**: `CombineOp` is fixed at `initialize` time and cannot be changed later by this contract's current interface, which forces the deployer to decide deliberately upfront rather than silently drifting between AND and OR. Document the semantics clearly for operators at deployment time.

#### Threat 9.4.3: Event-Preserving Failure Design Relies on the Caller Checking the Return Value
**Attacker capability**: N/A directly — an integration risk.

**Impact**: Because `evaluate` returns `Ok(false)` rather than an `Err` on a failed policy (by design — see Assumption in 9.2), a calling contract that only checks `is_err()` rather than inspecting the returned `bool` would incorrectly treat a *failed* compliance decision as a *successful* call, and proceed with whatever action the policy was meant to gate.

**Mitigation**: This is the natural cost of preserving the audit event on failure, and is documented in the module's doc comment. Integrators must be told explicitly (in their own integration docs / code review) to branch on the returned `bool`, not just the `Result`'s `Ok`/`Err` variant — this is the single most important integration detail for this contract.

### 9.5 Test Coverage Summary

| Invariant | Test | File |
|-----------|------|------|
| 9.3.1 | `test_all_checks_pass`, `test_one_check_fails_and_semantics` | [`policy-engine/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/policy-engine/src/test.rs) |
| 9.3.2 | `test_one_check_passes_or_semantics` | [`policy-engine/src/test.rs`](https://github.com/stellar-compliance-kit/compliance-primitives/blob/main/contracts/policy-engine/src/test.rs) |
| 9.3.3 | *GAP* — no explicit empty-list `Any` test | — |
| 9.3.4 | *GAP* — no explicit event-emission test (existing tests don't assert on `env.events()`) | — |
| 9.3.5 | *GAP* — no explicit non-admin-rejection test for `add_check`/`remove_check` | — |
| 9.3.6 | *GAP* — no explicit double-initialize test | — |

---

## 10. Cross-Contract Composition (RWA Token Example)

The `/examples/rwa-token` contract demonstrates how to compose all three primitives into a single transfer function.

### 10.1 Composition Invariant

**Statement**: A transfer `rwa_token.transfer(A, B, amount)` succeeds iff:
1. Denylist check passes for both A and B
2. Allowlist check passes for both A and B
3. Jurisdiction check passes for both A and B
4. A has sufficient balance

**Proof**: Lines 106-145 of `examples/rwa-token/src/lib.rs` implement these checks in sequence, returning early if any fails.

**Tests verifying this invariant**:
- `examples/rwa-token/src/test.rs:test_transfer_fails_if_denied_by_denylist`
- `examples/rwa-token/src/test.rs:test_transfer_fails_not_on_allowlist`
- `examples/rwa-token/src/test.rs:test_transfer_fails_insufficient_balance`
- `examples/rwa-token/src/test.rs:test_transfer_succeeds_when_not_denied`

### 10.2 Check Order Invariant

**Statement**: Checks are performed in order: denylist → allowlist → jurisdiction → balance. If an earlier check fails, later checks are not executed (short-circuit evaluation).

**Proof**: Lines 118-121 (denylist), 124-128 (allowlist), 131-137 (jurisdiction), 140-143 (balance) perform checks sequentially with early returns.

**Tests verifying this invariant**:
- `examples/rwa-token/src/test.rs:test_transfer_multiple_checks_fail_in_order` — multiple checks fail, first failure is returned

---

## 11. Cross-Contract Invariants of the Composition Contracts

`compliance-aggregator` and `policy-engine` (sections 6 and 9) don't just
call `denylist-gate` and `jurisdiction-flag` — they each make a specific
promise about *what the combined result means* relative to the primitives
they wrap. This section states those promises explicitly, since they're the
properties an auditor of a *deployment* (not just of one contract in
isolation) needs to verify hold end to end.

#### Cross-Contract Invariant 11.1: Neither Composition Contract Can Be More Permissive Than Its Underlying Primitives Under AND Semantics
**Statement**: For `compliance-aggregator.check_address` (always AND) and `policy-engine.evaluate` with `op = All`, the composed result can only be `true` if **every individual primitive call the composition contract made** also returned `true`. Neither composition contract has any code path that overrides, ignores, or inverts a primitive's response while under AND semantics.

**Proof**: `compliance-aggregator`: Invariant 6.3.1 (`all_passed` only ever narrows via `&&`, never widens). `policy-engine`: Invariant 9.3.1 (`All` arm breaks to `all_pass = false` on the first failing primitive call and never resets it to `true`).

**Tests verifying this invariant**: see the test lists for Invariants 6.3.1 and 9.3.1.

#### Cross-Contract Invariant 11.2: Under OR Semantics, `policy-engine` Can Be More Permissive Than Any Single Primitive — By Design
**Statement**: `policy-engine.evaluate` with `op = Any` can return `true` even when a specific registered primitive returned `false` for one or both addresses, as long as at least one *other* registered primitive returned `true` for both. This is not a bug relative to the individual primitives' own invariants (each primitive still correctly reported its own `true`/`false`); it is the intended effect of composing them with OR. `compliance-aggregator` has no OR mode, so this invariant does not apply to it — see Invariant 6.2.

**Proof**: Invariant 9.3.2.

**Tests verifying this invariant**: `contracts/policy-engine/src/test.rs:test_one_check_passes_or_semantics`.

#### Cross-Contract Invariant 11.3: A Composition Contract's Result Is Only as Trustworthy as Its Registered Addresses
**Statement**: Neither `compliance-aggregator` nor `policy-engine` independently verifies that a registered contract address is a genuine deployment of `denylist-gate`/`jurisdiction-flag` (or implements their interface faithfully). The composed `true`/`false` result inherits whatever the registered address actually returns, which may not match the *real* `denylist-gate.check`/`jurisdiction-flag.is_permitted_jurisdiction` for that address if the registration was misconfigured or compromised.

**Proof**: `compliance-aggregator`: `DenylistGateClient::new(&env, &gate_addr)`/`JurisdictionFlagClient::new(&env, &flag_addr)` (`src/lib.rs:287`, `302` in `compliance-aggregator`) construct a client against whatever address is stored, with no on-chain verification of the callee's identity or bytecode. `policy-engine`: same pattern via `DenylistCheckClient`/`JurisdictionCheckClient` (`src/lib.rs:293`, `300` in `policy-engine`), constructed from the admin-supplied `contract` field on each `CheckKind`.

**Tests verifying this invariant**: not independently tested (both contracts' test suites use real, correctly-behaving mock/stub primitives — see Threats 6.4.1 and 9.4.1 for the corresponding threat-model treatment; there is no test that registers a deliberately misbehaving contract to confirm the composition faithfully surfaces its result either way).

---

## 12. Test Coverage Summary

### Tests by Category

| Category | Count | Details |
|----------|-------|---------|
| Denylist checks | 6 | Initialization, add/remove, check logic, authorization |
| Allowlist checks | 6 | Initialization, add/remove, transfer gating, authorization |
| Jurisdiction checks | 8 | Initialization, set/get, permitted codes, upgrade, authorization |
| RWA composition | 8 | Checks in isolation and combined, check order, balance |
| Audit-log | 6 | Initialization, record/read, authorization, event emission, out-of-range reads |
| Circuit-breaker | 3 | Default state, freeze/unfreeze round-trip, authorization |
| Compliance-aggregator | 19 | Initialization, admin management, AND-composition, partial registration, batched checks |
| Multisig-admin | 14 | Initialization, threshold validation, signer-set mutation, `__check_auth` threshold enforcement |
| Pausable (shared library) | 5 | Default state, pause/unpause round-trip, idempotency, `require_not_paused` |
| Policy-engine | 4 | AND/OR combination, check-list mutation |
| **Total** | **79** | — |

### Identified Gaps

1. **Jurisdiction event emission test** (Invariant 3.3.6) — No explicit test for `set_jurisdiction()` event emission. Recommend adding `test_set_jurisdiction_emits_event`.

2. **Allowlist event emission test** (Invariant 2.3.5) — May be missing explicit verification that events are emitted. Recommend adding if not present.

3. **Denylist event emission test** (Invariant 1.3.x) — Same as above.

4. **Concurrent/replay attack tests** — No tests for transaction replay or concurrent modifications. Soroban SDK handles nonce validation, but explicit tests would strengthen confidence.

5. **Upgrade event emission test** (Jurisdiction-flag) — `test_upgrade_emits_event` should verify the `UpgradePerformed` event is actually emitted.

6. **`audit-log`: `record()` before `initialize()`** (Invariant 4.3.1) — No explicit test that `record()` fails with `Error::NotInitialized` when called before `initialize()`.

7. **`circuit-breaker`: double-initialize** (Invariant 5.3.4) — No dedicated test, unlike the equivalent test present for every other contract in this workspace.

8. **`policy-engine`: four gaps** — no test for `Any` with zero registered checks (Invariant 9.3.3), no test asserting `PolicyResult` event emission via `env.events()` (Invariant 9.3.4), no non-admin-rejection test for `add_check`/`remove_check` (Invariant 9.3.5), and no double-initialize test (Invariant 9.3.6). Of the nine contracts in this workspace, `policy-engine` has the widest gap between its invariants and its explicit test coverage — recommend prioritizing these four before extending its check-kind support further.

---

## 13. Recommendations for External Auditors

1. **Verify storage layout stability**: Ensure that any contract upgrades maintain backward compatibility with existing persistent storage keys.

2. **Check authorization patterns**: All admin/issuer-gated functions use `require_auth()` correctly. Verify no authorization bypass exists.

3. **Verify cross-contract safety**: When these primitives are called from user contracts (e.g., RWA token), verify that return values are properly interpreted and that a false result truly blocks the transaction.

4. **Test edge cases**:
   - What happens if the underlying token in allowlist-token returns an error?
   - What happens if denylist-gate is uninitialized when called?
   - What happens if an address is set to an empty jurisdiction code?

5. **Review event emission**: Ensure all critical state changes emit events for off-chain observation.

6. **Verify no reentrancy vulnerabilities**: Soroban's model limits reentrancy, but verify that cross-contract calls don't allow attackers to manipulate state during a transaction.

7. **Verify registered composition-contract addresses independently**: for `compliance-aggregator` and `policy-engine`, confirm the on-chain `DenylistGate`/`JurisdictionFlag`/`CheckKind.contract` addresses actually point at genuine deployments of the expected contracts before trusting an aggregated `true` result — see Cross-Contract Invariant 11.3.

8. **Confirm `policy-engine`'s `CombineOp` matches deployment intent**: since it cannot be changed after `initialize()`, verify the deployed value (`All` vs `Any`) matches the issuer's actual compliance policy, especially for `Any`, which is strictly more permissive than any individual registered check — see Threat 9.4.2.

9. **Confirm `multisig-admin`'s `threshold`-to-`signers.len()` ratio matches the intended security margin** for each deployment that uses it as a primitive's admin — see Threat 7.4.1.

10. **Confirm `pausable` integration is complete on every contract that claims it**: for each of `allowlist-token`, `denylist-gate`, and `jurisdiction-flag`, verify `require_not_paused` is actually called at the top of every state-mutating method, not just declared as a dependency in `Cargo.toml` — see [`ARCHITECTURE.md`](ARCHITECTURE.md#shared-library-pausable).

---

## Appendix: Glossary

- **Invariant**: A property that must hold true before and after any function call.
- **Threat**: A potential attack vector or failure mode.
- **Mitigations**: Controls that reduce the likelihood or impact of a threat.
- **Test coverage gap**: An invariant or threat that is not explicitly tested.
- **Short-circuit evaluation**: Early return from a function once a condition is met, skipping later checks.

