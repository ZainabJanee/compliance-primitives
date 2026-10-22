# Incident Response: Compromised Admin Key

Closes #464.

This runbook covers what to do when the admin (or issuer) key for one or more
compliance-primitive contracts is believed to be compromised.  It is structured
in two phases: **immediate mitigation** (things you can do in minutes to limit
damage) and **recovery** (the longer-term path to restoring safe operation per
contract).

---

## 0. Triage — what is exposed?

Before acting, determine which contracts the compromised key controls:

| Key role | Controls |
|---|---|
| `allowlist-token` admin | Can add/remove allowlist entries, pause/unpause, upgrade contract |
| `denylist-gate` admin | Can add/remove denylist entries, pause/unpause, propose/commit upgrades |
| `jurisdiction-flag` issuer | Can set/remove jurisdiction codes, pause/unpause, upgrade contract |
| `circuit-breaker` admin | Can freeze/unfreeze, propose/accept new admin |
| `compliance-aggregator` admin | Can reconfigure which gate/flag/breaker contracts are consulted |
| `policy-engine` admin | Can add/remove registered checks, set circuit-breaker, upgrade |
| `multisig-admin` signer | One signer in an M-of-N set; alone cannot act below threshold |

A compromised `multisig-admin` **single signer** is the least severe: the
attacker cannot unilaterally act unless your threshold is 1-of-N.  All other
cases should be treated as critical.

---

## 1. Immediate mitigation

### 1a. Freeze via circuit-breaker (if configured)

If a `circuit-breaker` contract is wired into your token/aggregator/policy-engine
and you still have access to its admin key (which may differ from the compromised
key), **freeze immediately**:

```sh
stellar contract invoke \
  --id <CIRCUIT_BREAKER_CONTRACT_ID> \
  --source <CIRCUIT_BREAKER_ADMIN_KEY> \
  --network mainnet \
  -- freeze --admin <CIRCUIT_BREAKER_ADMIN_ADDRESS>
```

This causes every consumer that calls `is_frozen()` before transfers to return
`true`, blocking all gated operations.  It does **not** prevent the attacker
from continuing to mutate allowlist/denylist state, but it stops transfers from
clearing the compliance gate while you respond.

Verify the freeze took effect:

```sh
stellar contract invoke \
  --id <CIRCUIT_BREAKER_CONTRACT_ID> \
  --source <any-read-only-key> \
  --network mainnet \
  -- is_frozen
# expected output: true
```

### 1b. Pause the affected contract (if pause/unpause is available)

All contracts except `multisig-admin` support a `pause` admin function.  If the
compromised key is the admin/issuer and you have **another** authorized path to
the pause function (e.g. a `multisig-admin` wrapper, or a compliance officer key
for `jurisdiction-flag`), pause the contract:

```sh
# For denylist-gate
stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network mainnet \
  -- pause --admin <ADMIN_ADDRESS>

# For jurisdiction-flag (issuer role)
stellar contract invoke --id <CONTRACT_ID> --source <ISSUER_KEY> --network mainnet \
  -- pause --issuer <ISSUER_ADDRESS>

# For allowlist-token
stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_KEY> --network mainnet \
  -- pause --admin <ADMIN_ADDRESS>
```

**Caveats**:
- `denylist-gate::check` continues to work while paused — denylist reads are
  unaffected by pause state.  Only writes (add/remove) are blocked.
- If the compromised key *is* the only admin, you cannot pause; skip to §2.

### 1c. Rotate the admin key via `propose_admin` / `accept_admin` (allowlist-token, circuit-breaker)

`allowlist-token` and `circuit-breaker` both implement a two-step admin rotation
(`propose_admin` + `accept_admin`).  If the key is suspected compromised but you
still have control, immediately rotate to a fresh key:

```sh
# Step 1: propose the new admin (current admin signs)
stellar contract invoke --id <CONTRACT_ID> --source <CURRENT_ADMIN_KEY> \
  --network mainnet \
  -- propose_admin \
     --admin <CURRENT_ADMIN_ADDRESS> \
     --new_admin <NEW_ADMIN_ADDRESS>

# Step 2: accept (new admin signs) — can be a separate transaction
stellar contract invoke --id <CONTRACT_ID> --source <NEW_ADMIN_KEY> \
  --network mainnet \
  -- accept_admin --new_admin <NEW_ADMIN_ADDRESS>
```

This immediately transfers control to the new key.  The old compromised key can
no longer issue admin calls once `accept_admin` completes.

### 1d. Alert off-chain monitoring

Regardless of which on-chain steps you can take:

- Notify your compliance officer and legal team immediately.
- Flag the compromised key in your internal key-management system.
- Export the full on-chain event history for the affected contract since the key
  was last known safe (use the `/tools/indexer` or Horizon API), and review every
  `DenyAdd`, `DenyRemove`, `AllowAdd`, `AllowRemove`, `JurisdictionSet`,
  `JurisdictionRemoved`, `Frozen`, and `Unfrozen` event.

---

## 2. Recovery per contract

### `allowlist-token`

**Has upgrade path**: yes — `propose_upgrade` / `commit_upgrade` (two-step,
time-delayed).

**Preferred recovery path**:
1. Rotate admin immediately via `propose_admin` / `accept_admin` (§1c above).
2. Audit all `AllowAdd` / `AllowRemove` events from the compromise window.
3. Re-add any addresses incorrectly removed; remove any fraudulently added.
4. Unpause (if paused) once state is verified clean.

**If you cannot rotate** (compromised key is the only admin and attacker has
acted first):
- Full redeploy is required.  See §3.

---

### `denylist-gate`

**Has upgrade path**: yes — `propose_upgrade` / `commit_upgrade` (two-step,
time-delayed).

**Preferred recovery path**:
1. If another admin path exists (e.g. `multisig-admin` wrapper), use it to call
   `pause` and then rotate or redeploy.
2. Audit all `DenyAdd` / `DenyRemove` events from the compromise window.
3. Re-deny any addresses fraudulently removed; remove any fraudulently added
   entries.
4. Unpause once state is verified.

**Note**: `denylist-gate::check` is **not** affected by `pause` — read operations
continue.  Pausing only blocks further admin mutations.

**If you cannot rotate**:
- Full redeploy required.  See §3.

---

### `jurisdiction-flag`

**Has upgrade path**: yes — `upgrade(issuer, new_wasm_hash)` (single-step,
issuer-only; see threat model J6 — this is less safe than the two-step variant).

**Preferred recovery path**:
1. If a compliance officer key exists and is not compromised, use it to audit
   state (compliance officers can call `set_jurisdiction`).
2. Rotate the issuer key if possible by redeploying or using a multisig wrapper.
3. Audit all `JurisdictionSet` / `JurisdictionRemoved` events from the
   compromise window.
4. Correct any jurisdiction codes changed by the attacker.

**If you cannot rotate**:
- Full redeploy required.  See §3.

---

### `circuit-breaker`

**Has upgrade path**: no explicit WASM upgrade function in the current
implementation.

**Has admin rotation**: yes — `propose_admin` / `accept_admin`.

**Preferred recovery path**:
1. Rotate the admin key immediately via `propose_admin` / `accept_admin` (§1c).
2. Verify the frozen/unfrozen state is correct: if the attacker called
   `unfreeze`, re-freeze via the new admin key.
3. If you cannot rotate (attacker rotated admin away from you), deploy a fresh
   `circuit-breaker` and update all consumers (`compliance-aggregator`,
   `policy-engine`, your token contract) to point to the new instance.

---

### `compliance-aggregator`

**Has upgrade path**: no explicit WASM upgrade function.

**Preferred recovery path**:
1. Rotate admin via `set_admin` if not yet overwritten.
2. Audit whether the attacker reconfigured the registered gate/flag/breaker
   addresses (check `DenylistGateSet`, `JurisdictionFlagSet`,
   `CircuitBreakerSet` events).
3. Restore correct contract addresses via `set_denylist_gate`,
   `set_jurisdiction_flag`, `set_circuit_breaker`.

**If admin is overwritten**:
- Full redeploy required.  See §3.

---

### `policy-engine`

**Has upgrade path**: yes — `propose_upgrade`, then `commit_upgrade` after
the configured ledger delay; `cancel_upgrade` stops a pending upgrade.

**Preferred recovery path**:
1. Pause if possible.
2. Audit registered checks (`get_checks`) — the attacker could have added
   a rogue denylist/jurisdiction/allowlist contract address as a check,
   or removed a required check.
3. Restore the correct check set via `add_check` / `remove_check`.
4. If the admin is overwritten: full redeploy.  See §3.

---

### `multisig-admin`

**Compromised single signer** (threshold > 1):
- If the remaining honest signers still meet the threshold, they can collectively
  call `remove_signer` to remove the compromised signer, then `add_signer` to
  add a replacement.
- No immediate access to any primitive is lost unless the attacker has access to
  enough other signers to meet threshold.

**Threshold met by attacker** (attacker controls ≥ threshold signers):
- Treat as a full admin compromise of every primitive that uses this
  `multisig-admin` as its admin address.
- Immediately freeze via circuit-breaker (if controlled by a separate key).
- Full redeploy of affected primitives required.  See §3.

---

## 3. Full redeploy procedure

When admin rotation is not possible (attacker has already overwritten the admin
key, or the contract has no rotation function), the recovery path is a full
redeploy:

1. **Deploy new contract instances** for each affected primitive.
2. **Migrate state**:
   - For `denylist-gate`: replay all `DenyAdd` events that were *not* reversed
     by a `DenyRemove`, minus any fraudulent adds from the compromise window.
   - For `allowlist-token`: replay all `AllowAdd` events that were not reversed,
     minus fraudulent adds.
   - For `jurisdiction-flag`: replay all `JurisdictionSet` events (keeping the
     last write per address), minus fraudulent changes.
   - See [docs/MIGRATION.md](./MIGRATION.md) for the general migration pattern.
3. **Update consumers**: any contract that holds a hardcoded or stored address
   for the affected primitive (token contract, `compliance-aggregator`,
   `policy-engine`) must be updated to point at the new deployment.
4. **Unfreeze** the circuit-breaker (if you froze in §1a) once the new
   contracts are verified and consumers updated.
5. **Revoke / burn the compromised key** in your key management system.

---

## 4. Post-incident

- Prepare a post-mortem covering how the key was compromised, which state was
  affected, and what transfers (if any) went through that should not have.
- If any unauthorized transfers occurred, this is likely a reportable compliance
  event under your regulatory framework.
- Consider migrating all affected primitives to a `multisig-admin` setup so that
  future single-key compromises cannot unilaterally affect compliance state.
- File a vulnerability report via [SECURITY.md](../SECURITY.md) if the
  compromise exposed a flaw in the contracts themselves (rather than purely in
  your key management).
