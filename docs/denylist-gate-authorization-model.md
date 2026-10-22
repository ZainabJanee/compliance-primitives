# Denylist-Gate Authorization Model

`denylist-gate` supports direct single-admin governance and composition with the
standalone `multisig-admin` contract. The compliance officer is a delegated
operational role, not a governance role.

## Authority matrix

| Operation | Single admin | Officer configured | Multisig-backed admin | Paused |
| --- | --- | --- | --- | --- |
| `check`, `is_denylisted`, and views | Public read | Public read | Public read | Works |
| `add_to_denylist`, `remove_from_denylist`, `remove_multiple_from_denylist` | Admin | Admin or officer | The multisig authorizes the configured admin address | Rejected |
| `set_compliance_officer`, `revoke_compliance_officer` | Admin only | Admin only | Multisig authorization of admin | Available to admin |
| `set_audit_log` | Admin only | Admin only | Multisig authorization of admin | Available to admin |
| `pause`, `unpause` | Admin only | Admin only | Multisig authorization of admin | Pause blocks denylist mutations |
| `initialize_multisig` | Current admin once | Current admin once | Not applicable after conversion | Available to admin |

The officer cannot assign or revoke itself, change the audit-log destination,
change signer governance, or pause the contract. It can add or remove denylist
entries, individually or in batch (`remove_multiple_from_denylist`, up to 45
addresses per call), while the contract is not paused. Single and batch removal
use the same authority check, so a batch call never fails for an officer where
the equivalent sequence of single calls would succeed.

## Multisig composition

`multisig-admin` is used as the `admin` address during deployment or migration.
When the gate calls `admin.require_auth()`, Soroban invokes the multisig
contract's M-of-N authorization hook. Signer membership and threshold
validation therefore belong to `multisig-admin`; the gate does not duplicate
signature counting. The officer role is additive and must not be treated as a
bypass for governance changes.

## Audit log and storage lifetime

Audit logging is opt-in. After `set_audit_log`, each successful individual add
or remove records an entry whose source is the gate contract. Without a
configured destination, no cross-contract call is made.

`Admin`, `Paused`, `ComplianceOfficer`, `SignerSet`, and `AuditLog` are instance
storage keys. Soroban bumps the instance entry TTL when the contract is
invoked, so explicit per-operation TTL extension is unnecessary for the
expected continuously operated deployment. Denylist entries remain persistent
and receive an explicit long TTL extension because their reads can occur
independently of instance-storage activity.

Pause affects mutations only. Read-only compliance checks continue to work
while paused, allowing consumers to fail closed or apply their own pause policy.
