# Threat Model: Compliance-Officer Delegation Pattern

<!-- Resolves #459 -->

## Overview

Three contracts in this workspace implement a **compliance-officer delegation**
pattern: `allowlist-token`, `denylist-gate`, and `jurisdiction-flag`. In each
contract the primary privileged role (called `admin` in `allowlist-token` /
`denylist-gate`, and `issuer` in `jurisdiction-flag`) may designate a second
address — the **compliance officer** — that receives a limited, delegated
subset of write authority.

This document defines the intended security properties of that pattern,
analyses each implementation against those properties, and records any gaps
found.

---

## Intended Security Properties

### P1 – Privilege containment

A compliance officer MUST NOT be able to escalate its own privileges to those
of the admin/issuer. Specifically:

- An officer MUST NOT be able to call `set_compliance_officer` or
  `revoke_compliance_officer` to replace itself with another address or
  remove its own revocability.
- An officer MUST NOT be able to call admin-only lifecycle operations
  (pause/unpause, upgrade, admin transfer).
- An officer MUST NOT be able to add addresses to an allowlist or grant any
  access that only the admin may grant.

### P2 – Revocability

The admin/issuer MUST be able to revoke the compliance-officer role at any
time by calling the appropriate revocation function, without co-operation
from the officer. Revocation MUST take effect immediately and atomically.

### P3 – Key-compromise recovery

If a compliance-officer key is compromised, the admin MUST be able to
recover by:

1. Calling `revoke_compliance_officer` (admin-only) to strip the compromised
   key of all delegated authority; and
2. Optionally calling `set_compliance_officer` with a replacement key.

The admin MUST NOT need the officer's co-operation or signature to accomplish
step 1. The compromise of an officer key MUST NOT allow an attacker to
prevent the admin from revoking it (e.g. by pausing the contract or
transferring the admin role).

### P4 – Audit trail

Set and revoke operations for the compliance-officer role SHOULD be
observable on-chain (via storage reads or events) so that an auditor can
reconstruct the full history of delegation changes.

### P5 – Scope clarity

The exact set of entry points available to the compliance officer (vs.
admin-only entry points) MUST be documented and enforced uniformly. The
pattern across contracts SHOULD be consistent.

---

## Implementations Audited

### `allowlist-token`

**Officer-accessible entry points:**
- `remove_from_allowlist` (via `require_compliance_authority`)

**Admin-only entry points (officer cannot call):**
- `add_to_allowlist`, `add_to_allowlist_delegated`
- `set_compliance_officer`, `revoke_compliance_officer`
- `pause`, `unpause`
- `propose_upgrade`, `commit_upgrade`, `cancel_upgrade`
- `propose_admin`, `accept_admin`, `transfer_admin`

**Assessment against properties:**

| Property | Status | Notes |
|---|---|---|
| P1 – Privilege containment | ✅ Pass | `require_compliance_authority` only covers `remove_from_allowlist`; all grant operations remain strictly admin-only. |
| P2 – Revocability | ✅ Pass | `revoke_compliance_officer` is admin-only and removes the `ComplianceOfficer` storage key immediately. |
| P3 – Key-compromise recovery | ✅ Pass | Admin can call `revoke_compliance_officer` unilaterally; officer cannot pause or transfer admin. |
| P4 – Audit trail | ⚠️ Partial | Set/revoke operations write to instance storage and are readable, but no dedicated event is emitted. Consider adding `ComplianceOfficerSet` / `ComplianceOfficerRevoked` events. |
| P5 – Scope clarity | ✅ Pass | Module-level doc comment explicitly lists what the officer may and may not do. |

**Follow-up issue:** Emit dedicated events for officer set/revoke for auditability (P4 gap).

---

### `denylist-gate`

**Officer-accessible entry points:**
- `add_to_denylist` (via `require_compliance_authority`)
- `remove_from_denylist` (via `require_compliance_authority`)
- `remove_multiple_from_denylist` (via `require_compliance_authority`)

**Admin-only entry points (officer cannot call):**
- `set_compliance_officer`, `revoke_compliance_officer`
- `set_audit_log`
- `pause`, `unpause`
- `propose_upgrade`, `commit_upgrade`, `cancel_upgrade`
- `transfer_admin`
- `initialize_multisig`, `add_signer`, `remove_signer`

**Assessment against properties:**

| Property | Status | Notes |
|---|---|---|
| P1 – Privilege containment | ✅ Pass | Officer can add and remove individual denylist entries but cannot manage roles, pause, or upgrade. |
| P2 – Revocability | ✅ Pass | `revoke_compliance_officer` is admin-only and removes the key immediately. |
| P3 – Key-compromise recovery | ✅ Pass | Admin can revoke unilaterally; a compromised officer cannot pause the contract to block revocation. |
| P4 – Audit trail | ⚠️ Partial | No dedicated set/revoke event for the officer role. The `get_compliance_officer` read accessor exists. |
| P5 – Scope clarity | ✅ Pass | Module and function docs describe the split clearly. |

**Follow-up issue:** Emit dedicated events for officer set/revoke (P4 gap).

---

### `jurisdiction-flag`

**Officer-accessible entry points:**
- `set_jurisdiction` (via `require_compliance_authority`)
- `set_jurisdiction_until` (via `require_compliance_authority`)
- `add_jurisdiction` (via `require_compliance_authority`)

**Issuer-only entry points (officer cannot call):**
- `remove_jurisdiction`, `remove_jurisdiction_multiple`
- `set_compliance_officer`, `revoke_compliance_officer`
- `transfer_issuer`
- `pause`, `unpause`
- `upgrade`

**Assessment against properties:**

| Property | Status | Notes |
|---|---|---|
| P1 – Privilege containment | ✅ Pass | Officer can set/add jurisdiction codes but cannot remove them in bulk, manage roles, pause, or upgrade. |
| P2 – Revocability | ✅ Pass | `revoke_compliance_officer` is issuer-only and removes the key immediately. |
| P3 – Key-compromise recovery | ✅ Pass | Issuer can revoke unilaterally. Officer cannot pause or transfer the issuer role. |
| P4 – Audit trail | ⚠️ Partial | No events for officer set/revoke. |
| P5 – Scope clarity | ✅ Pass | Both `require_issuer` and `require_compliance_authority` helpers are documented with explicit scope explanations in their doc comments. |

**Follow-up issue:** Emit dedicated events for officer set/revoke (P4 gap).

---

## Cross-Contract Consistency

The three implementations are **functionally consistent** on the security
properties that matter most (P1–P3). Minor differences are intentional and
reflect each contract's role:

- `denylist-gate` grants the officer both add and remove authority (adding
  to a denylist is a protective action, not a privilege grant, so this is
  appropriate).
- `allowlist-token` restricts the officer to remove-only (adding to an
  allowlist grants transacting rights, so it stays admin-only — by design).
- `jurisdiction-flag` grants the officer set/add authority (assigning
  jurisdiction codes is routine compliance work, not a security-sensitive
  grant).

All three implement `revoke_compliance_officer` as a single admin/issuer-only
call that immediately removes the key from storage.

---

## Follow-up Issues Filed

The following gaps were identified during this audit. They should be filed as
separate issues and are non-blocking for the current threat model:

| Contract | Gap | Severity |
|---|---|---|
| `allowlist-token` | No event emitted on `set_compliance_officer` / `revoke_compliance_officer` | Low |
| `denylist-gate` | No event emitted on `set_compliance_officer` / `revoke_compliance_officer` | Low |
| `jurisdiction-flag` | No event emitted on `set_compliance_officer` / `revoke_compliance_officer` | Low |

---

## Threat Scenarios

### T1 – Compromised officer key attempts to escalate

**Threat:** An attacker who controls a compliance-officer key calls
`set_compliance_officer` to replace the officer with their own key, or
calls admin-only functions to gain full control.

**Mitigation:** All three contracts gate `set_compliance_officer`,
`revoke_compliance_officer`, `pause`, `upgrade`, and admin/issuer transfer
on the strict `require_admin` / `require_issuer` helper, not
`require_compliance_authority`. The officer address has no path to these
entry points.

**Residual risk:** None. The design explicitly documents and enforces this split.

### T2 – Admin key lost, officer key still active

**Threat:** The admin/issuer key is lost. The compliance officer continues
operating but the delegation can no longer be revoked.

**Mitigation:** Each contract implements a two-step admin transfer
(`propose_admin` / `accept_admin` on `allowlist-token`, `transfer_admin`
on `denylist-gate`, `transfer_issuer` on `jurisdiction-flag`). If the admin
key is lost and these paths are unavailable, the officer key remains active
indefinitely — this is an operational risk, not a contract bug.

**Recommendation:** Deployers should use `multisig-admin` as the admin
address so that loss of one key does not equal loss of the admin role.

### T3 – Compromised officer key used for malicious compliance changes

**Threat:** An attacker with an officer key adds malicious addresses to a
denylist (`denylist-gate`), sets incorrect jurisdiction codes
(`jurisdiction-flag`), or removes legitimate addresses from an allowlist
(`allowlist-token`).

**Mitigation (contract level):** The contract can detect misuse by
watching on-chain storage and events. The admin can revert the changes
(re-add to allowlist, correct jurisdiction codes, remove from denylist) and
then revoke the officer.

**Residual risk:** The attacker can cause compliance disruption for the
window between key compromise and admin revocation. Deployers should monitor
for unexpected compliance changes and have a runbook for rapid revocation.
See `docs/incident-response.md`.

### T4 – Officer role left permanently assigned after usage

**Threat:** An operator assigns the compliance-officer role for a one-off
batch task and forgets to revoke it. The standing delegation becomes an
unnecessary attack surface.

**Mitigation (operational):** Deployers should revoke the officer role
immediately after the delegated task is complete. Consider integrating an
automated check into the pre-mainnet checklist (see
`docs/pre-mainnet-checklist.md`) that flags standing officer delegations
before go-live.

---

*Document authored as part of issue #459. For vulnerability reporting, see
`SECURITY.md`.*
