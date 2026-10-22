# Cross-Contract Trust Assumptions Audit

<!-- Resolves #460 -->

## Purpose

This document audits every `#[contractclient]`-based cross-contract call site
in the workspace for reentrancy risk and malicious-callee scenarios, and
documents the trust model at each composition point.

The guiding question for each call site is:

> _If the configured target address is a malicious or misconfigured contract,
> what is the worst-case outcome, and is that outcome bounded?_

---

## Call Sites Audited

### 1. `denylist-gate` → `audit-log` (`AuditLogClient`)

**Contract:** `denylist-gate/src/lib.rs`  
**Interface:** `AuditLogClient` (`record(source, kind, subject, detail)`)
  
**When called:** Optionally, after every successful `add_to_denylist` or
`remove_from_denylist` call, if a `DataKey::AuditLog` address has been
configured by the admin.

**Trust assumption:**
> The audit-log address is an admin-controlled configuration parameter
> (stored under `DataKey::AuditLog`, set via `set_audit_log(admin, ...)`)
> and is trusted to the same degree as any other admin-configured address.
> An admin who configures a malicious audit-log address is equivalent to an
> admin who misconfigures any other critical parameter.

**Reentrancy analysis:**
- The `record(...)` call is made at the *end* of the denylist mutation, after
  all storage writes and events in `denylist-gate` have already been applied.
  A re-entrant call back into `denylist-gate` from the audit-log callee would
  see fully committed state, not intermediate state.
- Soroban's execution model does not support reentrancy in the traditional
  EVM sense (there is no call stack with mid-execution state visible to
  callees); each cross-contract call proceeds with the calling contract's
  storage committed up to that point.
- **Reentrancy risk: Low.** The call is at the tail of the function body.
  A re-entrant denylist mutation would be a new, independent invocation with
  fresh auth requirements.

**Malicious-callee scenario:**
- A malicious `audit-log` contract could panic (trap), causing the
  `denylist-gate` transaction to roll back, effectively preventing
  `add_to_denylist` / `remove_from_denylist` from succeeding.
- **Worst case:** The admin has configured a DoS contract as the audit-log;
  all denylist mutations are blocked until the admin calls `set_audit_log`
  with a corrected address.
- **Blast radius:** Bounded to the denylist mutation functions. The `check()`
  read path does not call the audit-log and is unaffected.
- **Mitigation:** Admin must use `set_audit_log` to correct the address.
  A pre-deployment review checklist should verify the audit-log address
  before go-live (see `docs/pre-mainnet-checklist.md`).

---

### 2. `policy-engine` → `denylist-gate` (`DenylistCheckClient`)

**Contract:** `policy-engine/src/lib.rs`  
**Interface:** `DenylistCheckClient` (`check(address) -> bool`)

**When called:** During `evaluate` / `evaluate_verbose` / `batch_evaluate`,
for every registered `CheckKind::Denylist` check.

**Trust assumption:**
> The denylist-gate contract address is registered by the policy admin via
> `add_check(CheckKind::Denylist(...))`. It is trusted to the same degree as
> any admin-configured parameter. Replacing a legitimate denylist-gate with
> a malicious one requires the admin's authorization.

**Reentrancy analysis:**
- `check()` is called inside a loop within `evaluate`. A malicious callee
  could re-enter `policy-engine.evaluate`, but each re-entrant call would
  need to pass its own auth requirements and would operate on committed
  storage. No partial-write window is exploitable.
- **Reentrancy risk: Low.**

**Malicious-callee scenario:**
- A malicious denylist contract always returns `true` (allowing everyone)
  or always `false` (blocking everyone), or panics.
- Returning `true` always: policy evaluations that depend on a denylist
  check would incorrectly pass for all addresses — a compliance failure.
- Returning `false` always or panicking: policy evaluations fail for all
  addresses, acting as a DoS.
- **Worst case:** Compliance bypass (always-true) or DoS (panic/always-false).
- **Mitigation:** Admin key hygiene; multisig governance of the admin role
  (see `multisig-admin`); pre-mainnet review of all registered check addresses.

---

### 3. `policy-engine` → `jurisdiction-flag` (`JurisdictionCheckClient`)

**Contract:** `policy-engine/src/lib.rs`  
**Interface:** `JurisdictionCheckClient`
(`is_permitted_jurisdiction(address, allowed_codes) -> bool`)

**When called:** During evaluation, for every registered
`CheckKind::Jurisdiction` check. Called via `try_is_permitted_jurisdiction`
(returning `Ok(Ok(true))` on pass; any other result treats the check as
failed).

**Trust assumption:**
> Same as the denylist-gate call site: admin-controlled configuration,
> trusted to the degree of the admin who registered it.

**Reentrancy analysis:**
- Same pattern as the denylist-gate call — called inside the evaluation
  loop with no partially-committed policy-engine state visible to the callee.
- **Reentrancy risk: Low.**

**Malicious-callee scenario:**
- A malicious jurisdiction contract could bypass or block jurisdiction
  checks. The `try_...` call pattern means a panic in the callee is caught
  and treated as a failed check (`Ok(Ok(true))` is the only passing value),
  so a panicking callee causes the check to fail rather than propagating
  an unexpected error.
- **Worst case:** Compliance bypass (always-true return) if a malicious
  contract is registered. DoS is not possible via panic (trapped error = check fail).
- **Mitigation:** Same as above — admin key hygiene and governance.

---

### 4. `policy-engine` → `circuit-breaker` (`CircuitBreakerClient`)

**Contract:** `policy-engine/src/lib.rs`  
**Interface:** `CircuitBreakerClient` (`is_frozen() -> bool`)

**When called:** At the start of `evaluate` / `evaluate_verbose` (emergency
short-circuit), and for registered `CheckKind::CircuitBreaker` checks.

**Trust assumption:**
> Admin-controlled configuration. A malicious circuit-breaker address can
> affect all evaluations that pass through `policy-engine`.

**Reentrancy analysis:**
- `is_frozen()` is a pure read; it does not modify state. Re-entrancy from
  this call has no meaningful impact.
- **Reentrancy risk: Negligible.**

**Malicious-callee scenario:**
- Always returns `true` (always frozen): `evaluate` always returns `false`
  — a complete DoS of the policy engine.
- Always returns `false` (never frozen): the emergency stop is disabled;
  other registered checks still run.
- Panics: the transaction fails, blocking `evaluate`.
- **Worst case:** DoS of all policy evaluations.
- **Mitigation:** Admin governance; verify the circuit-breaker address in
  the pre-mainnet checklist.

---

### 5. `policy-engine` → `allowlist-token` (`AllowlistCheckClient`)

**Contract:** `policy-engine/src/lib.rs`  
**Interface:** `AllowlistCheckClient` (`is_allowed(address) -> bool`)

**When called:** During evaluation, for registered `CheckKind::Allowlist`
checks.

**Trust assumption:**
> Admin-controlled configuration. Same trust model as the denylist-gate
> and jurisdiction-flag call sites.

**Reentrancy analysis:**  
- Read-only call; no state changes. Re-entrancy negligible.

**Malicious-callee scenario:**
- Same as denylist-gate: always-true allows everyone; always-false or
  panic blocks everyone.
- **Worst case:** Compliance bypass or DoS.
- **Mitigation:** Admin governance.

---

### 6. `compliance-aggregator` → `denylist-gate` (`DenylistGateClient`)

**Contract:** `compliance-aggregator/src/lib.rs`  
**Interface:** `DenylistGateClient` (`check(address) -> bool`)

**When called:** In `check_address`, `check_all`, and `batch_check`, for
every address evaluated, if a denylist-gate address has been registered.

**Trust assumption:**
> Admin-controlled via `set_denylist_gate(admin, gate)`. Trusted to the
> same degree as any admin-configured parameter.

**Reentrancy analysis:**
- State writes in `compliance-aggregator` (none during evaluation — it only
  reads) precede the external call. No intermediate state is exposed.
- **Reentrancy risk: Low.**

**Malicious-callee scenario:**
- Same as `policy-engine → denylist-gate`.
- **Additional concern:** In `check_all` and `batch_check`, a malicious
  callee that panics would abort the entire batch. The blast radius is all
  addresses in the batch, not just one.
- **Mitigation:** Verify the registered gate address before go-live.

---

### 7. `compliance-aggregator` → `jurisdiction-flag` (`JurisdictionFlagClient`)

**Contract:** `compliance-aggregator/src/lib.rs`  
**Interface:** `JurisdictionFlagClient`
(`is_permitted_jurisdiction(address, allowed_codes) -> bool`)

**When called:** In `check_address`, `check_all`, and `batch_check`, via
`try_is_permitted_jurisdiction` (callee errors treated as check failure).

**Trust assumption:**
> Admin-controlled via `set_jurisdiction_flag(admin, flag)`. Same trust
> model as above.

**Reentrancy analysis:** Low (same reasoning as §3).

**Malicious-callee scenario:**
- Panic → check treated as failed (not a panic in aggregator) because of
  the `try_...` call pattern.
- Always-true → compliance bypass.
- **Worst case:** Compliance bypass.
- **Mitigation:** Admin governance.

---

### 8. `compliance-aggregator` → `circuit-breaker` (`CircuitBreakerClient`)

**Contract:** `compliance-aggregator/src/lib.rs`  
**Interface:** `CircuitBreakerClient` (`is_frozen() -> bool`)

**When called:** In `check_address`, `check_all`, and `batch_check` as a
pre-check via the private `is_frozen` helper.

**Trust assumption:**
> Admin-controlled via `set_circuit_breaker(admin, breaker)`. A
> compromised or malicious circuit-breaker affects all aggregator
> evaluations.

**Reentrancy analysis:** Negligible (pure read).

**Malicious-callee scenario:**
- Always frozen → all checks return failure, DoS of the aggregator.
- Always unfrozen → emergency stop disabled, other checks still run.
- **Worst case:** DoS.
- **Mitigation:** Admin governance.

---

## Summary Table

| Caller | Callee | Call | Reentrancy Risk | Malicious-Callee Worst Case | Trust Root |
|---|---|---|---|---|---|
| `denylist-gate` | `audit-log` | `record()` | Low | DoS of denylist mutations | Admin (`set_audit_log`) |
| `policy-engine` | `denylist-gate` | `check()` | Low | Compliance bypass or DoS | Admin (`add_check`) |
| `policy-engine` | `jurisdiction-flag` | `try_is_permitted_jurisdiction()` | Low | Compliance bypass (panic = fail) | Admin (`add_check`) |
| `policy-engine` | `circuit-breaker` | `is_frozen()` | Negligible | DoS of all evaluations | Admin (`set_circuit_breaker`) |
| `policy-engine` | `allowlist-token` | `is_allowed()` | Low | Compliance bypass or DoS | Admin (`add_check`) |
| `compliance-aggregator` | `denylist-gate` | `check()` | Low | Compliance bypass or DoS | Admin (`set_denylist_gate`) |
| `compliance-aggregator` | `jurisdiction-flag` | `try_is_permitted_jurisdiction()` | Low | Compliance bypass (panic = fail) | Admin (`set_jurisdiction_flag`) |
| `compliance-aggregator` | `circuit-breaker` | `is_frozen()` | Negligible | DoS of all evaluations | Admin (`set_circuit_breaker`) |

---

## Overall Findings

1. **No call site has an unbounded blast radius.** Every external call is
   made to an admin-configured address. Configuring a malicious address
   requires the admin's authorization (or a compromise of the admin key).

2. **Reentrancy risk is low across all call sites.** Soroban's execution
   model and the placement of cross-contract calls (after storage writes,
   or on read-only paths) limit reentrancy exposure.

3. **The `try_*` call pattern in `policy-engine` and `compliance-aggregator`
   is a meaningful defensive measure.** A panicking jurisdiction-flag callee
   is treated as a check failure rather than propagating an unexpected trap,
   preventing a malicious callee from causing an uncontrolled panic in the
   calling contract on the read path.

4. **The `denylist-gate → audit-log` path does not use `try_*`.** A
   panicking audit-log will abort a denylist mutation transaction. This is
   the most direct DoS vector in the workspace. Operators should verify the
   audit-log address before enabling it and monitor for unexpected failures.

5. **Admin key security is the root of trust for all call sites.** The
   recommended mitigation across all sites is to use `multisig-admin` as
   the admin address for every contract so that misconfiguring a contract
   address requires M-of-N approval.

---

*Document authored as part of issue #460. For vulnerability reporting, see
`SECURITY.md`.*
