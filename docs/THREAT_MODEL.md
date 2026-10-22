# Threat Model — Compliance Primitives

> Issue #112. This document enumerates and analyses **attack scenarios** for the
> three compliance-critical contracts: `allowlist-token`, `denylist-gate`, and
> `jurisdiction-flag`. It is distinct from SPEC.md (issue #29), which records
> per-contract invariants and security assumptions for audit reference. The two
> should be kept consistent: every invariant in SPEC.md should eventually have a
> corresponding scenario + mitigation here, and vice-versa.

## Scope

| Contract | Role |
|----------|------|
| `allowlist-token` | Wraps an underlying asset and gates `transfer` so only allowlisted addresses may send/receive. Admin-managed allowlist, pausable, two-step admin transfer. |
| `denylist-gate` | Maintains a denylist; `check(address)` is consulted before sensitive operations. Admin-managed, with an optional multisig signer set and a compliance-officer role. |
| `jurisdiction-flag` | Maps addresses → jurisdiction codes (with optional expiry). `is_permitted_jurisdiction` is the read gate. Issuer-managed, with a compliance-officer role and an `upgrade` entry point. |

## Rating scale

Severity × Likelihood → priority.

- **Severity**: Impact if the scenario succeeds (High = loss of funds / compliance bypass; Med = denial of service / griefing cost; Low = minor).
- **Likelihood**: How easily an external attacker (not the admin) can trigger it on Stellar/Soroban (High = no special access; Low = requires compromised/admin key).
- Mitigations are cross-referenced to a **function** (already implemented), a **test** (`contracts/<c>/src/test.rs`), or a **pending issue** (mitigation not yet landed).

---

## 1. `allowlist-token`

### Trust model
- `admin` controls the allowlist and can pause. Transferred via two-step
  `propose_admin` / `accept_admin` (#74/#75/#76).
- `transfer(from, to, amount)` is gated: both `from` and `to` must be allowlisted
  (`is_allowed`). 

### 1.1 Griefing
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| A1 | **Read-function resource-fee griefing** — `is_allowed` / `metadata` are public reads. An attacker repeatedly calls them to run up *their own* resource fees (Soroban charges the caller), so this only hurts the attacker; however, if a downstream contract calls `is_allowed` on behalf of a user, the griefer can force that contract to burn fees. | Low | High | None beyond Soroban's per-call fee model. Documented as accepted risk; consider caching/rate-limit in caller contracts. |
| A2 | **Denial via pause** — a compromised or reckless admin calls `pause`, freezing all transfers. | Med | Low | `unpause` by admin; two-step admin transfer (#74/#75/#76) limits how a key is swapped. Pause capability tracked in #84/#85. |
| A3 | **Allowlist churn** — admin removes a legit `from`/`to` mid-flight, causing in-progress transfers to fail. | Med | Low | `remove_from_allowlist` is admin-only; no non-admin write path exists, so storage cannot be filled by outsiders. |

**Storage-fill check**: `add_to_allowlist` / `remove_from_allowlist` are admin-only
(`admin` arg required + checked). There is **no** function that lets a non-admin
write a `DataKey`, so the "fill storage with junk entries" griefing class does
not apply. (Confirmed by reading `lib.rs`: all write entry points require `admin`.)

### 1.2 Front-running
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| A4 | **Add-vs-transfer ordering** — within a single ledger close, does an attacker benefit from `add_to_allowlist(victim)` landing before/after a `transfer`? `transfer` reads `is_allowed` at execution time, so a victim added then transferred in the same ledger is allowlisted for that transfer. This is the *intended* behavior (admin pre-authorizes). The risk is an admin racing a `remove_from_allowlist` *after* a queued `transfer` — the transfer still executes because it was already validated. | Med | Med | Acceptable: admin actions are authoritative by design. Mitigated by two-step admin (#74/#75/#76) and audit logging (issue #… audit-log contract). |
| A5 | **Two-step admin race** — `propose_admin` then `accept_admin` by the attacker before a legit `accept_admin`. Two-step design means the *new* admin must explicitly `accept_admin`, so an attacker cannot complete the transfer unilaterally. | Low | Low | `propose_admin`/`accept_admin` (#74/#75/#76). |

### 1.3 Admin-key compromise
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| A6 | Compromised `admin` adds attacker addresses to the allowlist, enabling illicit transfers of the wrapped asset. | High | Low | Bounded by `pause` (#84/#85) + two-step admin recovery (#74/#75/#76). Once #74/#75/#76 and #84/#85 land, a new admin can be proposed and the contract paused within one ledger. |
| A7 | Compromised `admin` calls `pause` permanently to extort/DoS. | Med | Low | Recovery requires admin key rotation (#74/#75/#76); consider a time-locked unpause or guardian (future issue). |

---

## 2. `denylist-gate`

### Trust model
- `admin` manages the denylist (`add_to_denylist`, `remove_from_denylist`,
  `remove_multiple_from_denylist`) and a `compliance_officer` role.
- Optional `multisig` signer set (`initialize_multisig`, `add_signer`,
  `remove_signer`) can gate admin actions.
- `check(address)` is the read gate consulted by integrators.

### 2.1 Griefing
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| D1 | **Read griefing** — `check` is a public read; same fee model as A1. Low impact (caller pays). | Low | High | Accepted; caller contracts should cache. |
| D2 | **Mass removal griefing** — `remove_multiple_from_denylist` by a compromised admin or compliance officer wipes the denylist, unblocking sanctioned addresses. | High | Low | `multisig` signers can require >1 approval on admin actions; `pause` (admin) halts integrators' `check` usage if they honor pause. Officer key compromise is not covered by multisig. |

**Storage-fill check**: all denylist writes are admin/compliance-officer only. No
non-admin write path → storage-fill griefing not applicable.

### 2.2 Front-running
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| D3 | **Denylist-vs-action ordering** — within a ledger close, an attacker may try to land a `transfer`/compliance-gated action *before* `add_to_denylist(attacker)` is committed. Conversely, a compliance officer removing an address right before a blocked action. Because `check` is evaluated at action execution, a denylist added in the same ledger blocks the action; a removal unblocks it. | High | Med | Integrators must call `check` and perform the gated action in the *same* transaction (atomic), or accept one-ledger TOCTOU. Recommend a `require_not_denied` hook invoked inside the gated entry point. Tracked as a future hardening issue. |
| D4 | **Multisig signer race** — `add_signer`/`remove_signer` by a single compromised signer. | Med | Low | Multisig threshold (set in `initialize_multisig`). |

### 2.3 Admin-key compromise
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| D5 | Compromised `admin` or `compliance_officer` removes sanctioned addresses → compliance bypass. | High | Low | Bounded by `multisig` (needs threshold of signers) and `pause`. Until #84/#85 pause lands for this contract, recovery depends on multisig threshold. |
| D6 | Compromised `admin` drains the signer set (`remove_signer`) to centralize control. | Med | Low | `multisig` threshold; propose a timelock on signer removal (future issue). |

---

## 3. `jurisdiction-flag`

### Trust model
- `issuer` sets jurisdiction codes (`set_jurisdiction`, `set_jurisdiction_until`,
  `remove_jurisdiction_multiple`) and holds the sole `upgrade` key.
- `compliance_officer` role can also set/revoke flags.
- `is_permitted_jurisdiction` is the read gate.
- **`upgrade(env, issuer, new_wasm)` already exists** — relevant to #113.

### 3.1 Griefing
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| J1 | **Read griefing** — `get_jurisdiction` / `is_permitted_jurisdiction` public reads; caller-pays. Low. | Low | High | Accepted. |
| J2 | **Expiry griefing** — `set_jurisdiction_until` with a past timestamp silently disables a flag, unblocking an address. | Med | Low | `issuer`/`compliance_officer`-only; pair with audit-log contract for review. |

**Storage-fill check**: all flag writes require `issuer` or `compliance_officer`.
No non-admin write path → storage-fill griefing not applicable.

### 3.2 Front-running
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| J3 | **Flag-vs-permit ordering** — within a ledger close, an attacker attempts a gated action before `set_jurisdiction(attacker, blocked)` commits, or a compliance officer clears a flag before a blocked action. Same TOCTOU as D3. | High | Med | Call `is_permitted_jurisdiction` atomically inside the gated action. Future hardening issue. |
| J4 | **Expiry race** — `set_jurisdiction_until` landing just before a check passes. | Med | Med | Atomic check recommended; `remove_jurisdiction_multiple` should be preferred for hard blocks. |

### 3.3 Admin-key compromise
| ID | Scenario | Sev | Lik | Mitigation |
|----|----------|-----|-----|------------|
| J5 | Compromised `issuer` rewrites jurisdiction flags to bypass jurisdiction rules. | High | Low | Recovery via `compliance_officer` + (pending) pause/#84/#85. Note: **`upgrade` is issuer-only with no multisig/timelock**, so a compromised `issuer` can also deploy arbitrary `new_wasm` — see J6. |
| J6 | **Compromised `issuer` calls `upgrade` with malicious `new_wasm`** → total contract takeover / fund theft in integrators. | Critical | Low | **No current mitigation.** This is exactly why #113 (upgradeability pattern / migration path) matters: a safe upgrade must be (a) two-step (propose + delay + accept), (b) timelocked so a watchguard can pause, and (c) ideally gated by `multisig-admin`. Until #113 lands, `upgrade` is a single-point-of-catastrophe key. |

---

## Cross-cutting observations

1. **No non-admin storage writes** in any of the three contracts — the
   storage-fill griefing class called out in the issue does not apply. Good.
2. **Front-running is the highest real risk** for all three: `check` /
   `is_permitted_jurisdiction` are read separately from the gated action, so a
   one-ledger TOCTOU window exists. Recommend an in-transaction hook
   (`require_not_denied` / `require_permitted`) rather than relying on callers to
   call the read then the action.
3. **Pause (#84/#85) and two-step admin (#74/#75/#76)** are the primary
   blast-radius limiters for admin compromise; several scenarios above assume
   they have landed. Track their merge before relying on A6/A7/D5/J5 mitigations.
4. **`jurisdiction-flag::upgrade` is the most dangerous single key** (J6). #113
   should introduce a delayed, reviewable upgrade before mainnet use.

## Open questions / follow-ups
- Should `check` / `is_permitted_jurisdiction` be made callable only as an
  in-transaction hook (remove the standalone read path)? 
- Do `allowlist-token` and `denylist-gate` also need an `upgrade` entry point
  (covered by #113 / #114)? They currently lack one.
- Confirm whether `denylist-gate` has a duplicate `add_to_denylist`
  declaration (observed at `lib.rs:161` and `lib.rs:189`) — if real, that is a
  compile error to fix separately.

*Severity/likelihood ratings are the author's best judgment from reading the
current `main` and should be reviewed by at least one other contributor before
merge, as required by the issue's acceptance criteria.*
