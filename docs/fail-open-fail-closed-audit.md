# Fail-Open vs Fail-Closed Audit: Read-Only Compliance Checks

Closes #463.

This document audits every read-only compliance check in the
`compliance-primitives` workspace for its behaviour under **storage archival**
(Soroban's TTL/expiry mechanism) and other **misconfiguration** scenarios,
classifying each as either **fail-open** (missing/expired entry → access
permitted) or **fail-closed** (missing/expired entry → access denied).

Understanding this distinction is safety-critical: a **fail-open** gate may
allow a non-compliant transfer to proceed silently when state has lapsed, while
a **fail-closed** gate may block a legitimate transfer.  Issuers must choose the
right primitive for their risk posture and ensure they maintain TTLs accordingly.

---

## Summary table

| Contract | Check function | Storage type | Archival behaviour | Classification |
|---|---|---|---|---|
| `denylist-gate` | `check(address)` | Persistent | Expired entry → treated as not-denied → **returns `true`** | ⚠️ **Fail-open** |
| `allowlist-token` | `is_allowed(address)` | Persistent | Expired entry → treated as not-allowlisted → **returns `false`** | ✅ **Fail-closed** |
| `jurisdiction-flag` | `is_permitted_jurisdiction(address, codes)` | Persistent | Expired entry → `None` → no code matches → **returns `false`** | ✅ **Fail-closed** |
| `circuit-breaker` | `is_frozen()` | Instance | Instance archived → `unwrap_or(false)` → **returns `false`** (not frozen) | ⚠️ **Fail-open** |
| `compliance-aggregator` | `check_address` / `check_all` / `batch_check` | Delegates | Inherits from composed primitives (see below) | Mixed |
| `policy-engine` | `evaluate(from, to)` | Delegates | Inherits from composed primitives (see below) | Mixed |

---

## Per-contract analysis

### `denylist-gate::check`

```rust
pub fn check(env: Env, address: Address) -> bool {
    !env.storage()
        .persistent()
        .get(&DataKey::Denied(address))
        .unwrap_or(false)     // ← absent / archived → false → check returns true
}
```

**Behaviour**: denylist entries are stored in **persistent** storage with a
maximum TTL (`MAX_TTL = 6_311_520` ledgers, ~1 year).  If an entry's TTL
expires and the ledger archives it, `get(…)` returns `None`, which
`unwrap_or(false)` silently converts to `false`.  The negation (`!false`)
yields `true` — meaning the address is treated as *clear to transact* even
though it may have been denied.

**Classification**: ⚠️ **Fail-open**

**Risk**: An attacker aware that a denylist entry has lapsed (or one who can
predict when it will lapse) can transact during that window without being
blocked.  This is the same footgun explicitly called out in the `denylist-gate`
module doc.

**Mitigation**:
- The `add_to_denylist` function sets `extend_ttl(THRESHOLD, MAX_TTL)` at write
  time, and the TTL is renewed on each write.  Admins should periodically call
  `add_to_denylist` again for long-lived entries (or use the batch variant), or
  operate a keeper that extends TTLs before they lapse.
- Monitor on-chain TTL values via off-chain indexing (the `/tools/indexer`
  component) and alert when any entry's remaining TTL drops below a safety
  threshold (e.g. 30 days).
- Consider treating an archived state as a compliance incident and triggering the
  circuit-breaker freeze while the entry is restored.

---

### `allowlist-token::is_allowed`

```rust
pub fn is_allowed(env: Env, address: Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Allowed(address))
        .unwrap_or(false)     // ← absent / archived → false → not allowed
}
```

**Behaviour**: allowlist entries are stored in **persistent** storage with a
90-day TTL (`ALLOWED_TTL_EXTEND_TO = 1_555_200` ledgers), renewed whenever
`add_to_allowlist` is called.  If an entry lapses, `get(…)` returns `None`,
which `unwrap_or(false)` maps to `false` — meaning the address is treated as
*not allowlisted* and the transfer is blocked.

**Classification**: ✅ **Fail-closed**

**Risk profile**: A legitimate user whose allowlist entry expires will have their
transfers blocked until the admin re-adds them.  This is the safer failure mode
for a permissioned-token gate — it may cause operational friction but will not
let an unapproved address through.

**Operational note**: The TTL is shorter here than for the denylist (90 days vs
~1 year).  Issuers should run a keeper process that calls `add_to_allowlist`
(which renews the TTL) before entries approach expiry, or lengthen
`ALLOWED_TTL_EXTEND_TO` to match their operational cadence.

---

### `jurisdiction-flag::is_permitted_jurisdiction`

```rust
pub fn is_permitted_jurisdiction(
    env: Env,
    address: Address,
    allowed_codes: Vec<String>,
) -> bool {
    match Self::get_jurisdiction(env, address) {
        Some(code) => allowed_codes.iter().any(|c| c == code),
        None => false,   // ← absent / archived → None → not permitted
    }
}
```

`get_jurisdiction` itself reads from **persistent** storage:

```rust
pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
    let key = DataKey::Jurisdiction(address);
    let code: Option<String> = env.storage().persistent().get(&key);
    if code.is_some() {
        Self::extend_jurisdiction_ttl(&env, &key);  // renew on read
    }
    code
}
```

**Behaviour**: jurisdiction flags are stored in **persistent** storage with a
short TTL (`TTL_EXTEND_TO = 5_000` ledgers, ~7 hours at 5s/ledger).  If a flag
lapses, `get_jurisdiction` returns `None`, and `is_permitted_jurisdiction`
returns `false` — blocking the transfer.

**Classification**: ✅ **Fail-closed**

**Risk profile**: The short TTL is a notable operational concern.  An address
whose jurisdiction flag has not been read recently will have it archived, and
subsequent compliance checks will treat it as having *no* jurisdiction — which
maps to "not permitted" under any allowed-codes list.  This is the safe failure
mode, but it will cause transfer failures for legitimate users if TTL
maintenance lapses.

**Mitigation**:
- `get_jurisdiction` auto-extends the TTL on each successful read (see above),
  so active addresses self-maintain their entries through normal usage.
- For addresses that transact infrequently, a keeper should call
  `get_jurisdiction` periodically to prevent archival.
- Issuers should consider whether `TTL_EXTEND_TO = 5_000` is appropriate for
  their network (at 5s/ledger this is ~7 hours on mainnet).  Increasing this
  constant reduces keeper-maintenance burden at the cost of more persistent
  storage rent.

---

### `circuit-breaker::is_frozen`

```rust
pub fn is_frozen(env: Env) -> bool {
    env.storage().instance().get(&DataKey::Frozen).unwrap_or(false)
}
```

**Behaviour**: the frozen flag is stored in **instance** storage.  Instance
storage is subject to TTL extension via `env.storage().instance().extend_ttl()`
in the contract's own flow, but `is_frozen` itself does not extend the TTL.  If
the circuit-breaker contract's instance is archived, `get(&DataKey::Frozen)`
returns `None`, and `unwrap_or(false)` maps to `false` — meaning the contract
behaves as if it is *not frozen*.

**Classification**: ⚠️ **Fail-open**

**Risk**: If the circuit-breaker contract's instance storage lapses (e.g. the
contract has not been interacted with for a long period), the frozen state is
silently lost and all consuming contracts that call `is_frozen()` will see
`false` and proceed as if no freeze is in effect.  This is particularly
dangerous if an issuer deploys a circuit-breaker and then has a quiet period —
the emergency stop may no longer be reliable.

**Mitigation**:
- Regularly call `is_frozen()` (or any other non-mutating call that causes the
  host to load the instance) to keep the TTL alive.  Alternatively, call
  `env.storage().instance().extend_ttl(…)` explicitly in a maintenance
  transaction.
- Add off-chain monitoring: alert if the circuit-breaker instance's TTL drops
  below a threshold (e.g. 30 days of ledgers).
- Consider treating a lapsed circuit-breaker as a compliance incident
  (equivalent to an unexpected unfreeze) and trigger an immediate manual review.
- For contracts where fail-closed behaviour on the freeze check is required,
  wrap the cross-contract call to `is_frozen()` in a pattern that treats a
  panicking/non-responsive call as *frozen* (though Soroban panics roll back the
  caller, so this requires careful design).

---

## `compliance-aggregator` composed behaviour

`check_address`, `check_all`, and `batch_check` each delegate to the configured
`denylist-gate` and/or `jurisdiction-flag` contracts via cross-contract calls.

The aggregator also consults a `circuit-breaker` if one is configured:

```rust
fn is_frozen(env: &Env) -> bool {
    match breaker_addr {
        Some(addr) => CircuitBreakerClient::new(env, &addr).is_frozen(),
        None => false,   // ← no breaker configured → not frozen
    }
}
```

**Inheritance of fail-open/fail-closed**:

| Component | Archival behaviour in aggregator |
|---|---|
| `denylist-gate.check` | Fail-open (see above) |
| `jurisdiction-flag.is_permitted_jurisdiction` | Fail-closed (see above) |
| `circuit-breaker.is_frozen` | Fail-open (see above); `None` breaker → not frozen |

**Misconfiguration**: if neither a denylist gate nor a jurisdiction flag is
registered, `check_address`/`check_all`/`batch_check` return
`Err(NoChecksRegistered)` rather than silently passing or failing.  This is a
fail-closed misconfiguration guard at the aggregation layer.

---

## `policy-engine` composed behaviour

`evaluate(from, to)` runs registered checks via `run_check`:

```rust
fn run_check(env: &Env, check: &CheckKind, address: &Address) -> bool {
    match check {
        CheckKind::Denylist(params) =>
            DenylistCheckClient::new(env, &params.contract).check(address),
        CheckKind::Jurisdiction(params) =>
            JurisdictionCheckClient::new(env, &params.contract)
                .is_permitted_jurisdiction(address, &params.allowed_codes),
        CheckKind::Allowlist(params) =>
            AllowlistCheckClient::new(env, &params.contract).is_allowed(address),
    }
}
```

**Inheritance**:

| Check kind | Archival behaviour in policy-engine |
|---|---|
| `Denylist` | Fail-open (delegates to `denylist-gate.check`) |
| `Jurisdiction` | Fail-closed (delegates to `jurisdiction-flag.is_permitted_jurisdiction`) |
| `Allowlist` | Fail-closed (delegates to `allowlist-token.is_allowed`) |

**`CombineOp::All` effect**: under `All`, *any* fail-open check that silently
returns `true` due to archival will not block a transfer on its own — only a
fail-closed check that returns `false` will block it.  This means an issuer
using `All` with only a denylist check is entirely fail-open on archival.

**`CombineOp::Any` effect**: under `Any`, a fail-open check returning `true`
(because its data was archived) may cause `evaluate` to return `true` even if a
fail-closed check would have returned `false`.  Issuers using `Any` should be
particularly careful that at least one check is fail-closed.

**Circuit-breaker in policy-engine**: the `circuit-breaker` consulted by
`evaluate` via `CircuitBreakerClient::is_frozen()` is subject to the same
fail-open instance-archival risk described above.  If the circuit-breaker's
instance lapses, the emergency short-circuit is silently disabled.

---

## `denylist-gate` mutations and the optional audit-log

`add_to_denylist` and `remove_from_denylist` append to an optional audit-log
configured via `set_audit_log`. The audit write is **fail-open**: it is issued
with `try_record`, and if it fails (the audit-log is paused, archived, or
reverting) the denylist mutation still succeeds and the gate emits
`AuditLogFailed { kind, subject }` instead.

**Why**: the audit-log is an opt-in, non-critical component. Making it
fail-closed would let a pause or archival of a *different* contract block the
gate's own admin from adding a sanctioned address. Sanctions entries must
remain writable during an audit-log incident.

**Consequence**: a denylist change can land without a matching audit entry.
Issuers who need every mutation recorded must watch for `AuditLogFailed`
events and reconcile the missed entries (replay `DenyAdd` / `DenyRemove` events
into the audit-log once it is restored). The `DenyAdd` / `DenyRemove` events are
the authoritative record of denylist state changes; the audit-log is a
secondary index.

**Not configurable (yet)**: there is no `strict_audit` switch. Issuers who want
fail-closed auditing must not configure an audit-log for now and must enforce
recording off-chain.

---

## Recommendations for issuers

1. **Treat denylist entries as high-priority TTL maintenance targets.** Their
   archival is silent and fail-open.  Operate a keeper or rely on the
   `/tools/indexer` to alert before any denylist entry's TTL drops below 30
   days.

2. **Monitor circuit-breaker instance TTL.** Its fail-open archival means an
   emergency freeze can be silently lost.  Prefer periodic `is_frozen()` calls
   or explicit TTL extension transactions.

3. **Allowlist and jurisdiction-flag entries are fail-closed** and will block
   legitimate transfers on archival rather than permit non-compliant ones.  This
   is safer but requires a keeper to prevent operational disruption.

4. **When composing with `policy-engine` under `CombineOp::Any`**, ensure at
   least one check is fail-closed (allowlist or jurisdiction), otherwise the
   entire policy is fail-open on archival of all denylist entries.

5. **Consider pairing every fail-open gate with an off-chain alert** that fires
   before archival occurs.  The on-chain contracts cannot self-alert; monitoring
   must be done externally.
