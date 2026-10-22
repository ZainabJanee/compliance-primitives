# Audit Log Retention, Archival, and Mirroring

<!-- Resolves #367 -->

This guide covers how long `audit-log` entries live on-chain, how their storage
grows, and how issuers should mirror them off-chain with `tools/indexer` before
they age out. It describes the contract as it is in `contracts/audit-log/src/lib.rs`
today. Where the current behavior falls short of the intended design, this
document says so.

---

## 1. What is stored

Every successful `record(source, kind, subject, detail)` call does two things:

1. Writes one **persistent** entry, `DataKey::Entry(index)`, holding a
   `LogEntry` (`source`, `kind`, `subject`, `detail`, `ledger`).
2. Increments `EntryCount` in **instance** storage, which becomes the next index.

It also publishes one `ComplianceEvent`. The indexer builds its `audit_log`
table from these events, not from the contract's storage.

Entries are never updated or removed by the contract. `SPEC.md` (Invariant
4.3.2) makes this append-only property a documented guarantee.

## 2. Expected storage growth

Storage grows linearly with the number of `record` calls, with no
compaction:

```
entries_per_year ≈ records_per_day × 365
bytes_per_entry  ≈ size of source + subject addresses + kind + detail + ledger + entry overhead
```

`detail` is the only field the caller can make large. Keep it short. Each
byte in `detail` is paid for on every entry.

Illustration only: at 1,000 records per day, the log holds about 365,000
entries after one year. Replace this with your own traffic estimate. Rent and
TTL-extension prices are set by network configuration, so check current
network values rather than relying on figures in this document.

Audit-log writes are meant for low-frequency compliance events such as
denylist mutations and jurisdiction overrides. The contract doc comment says
high-throughput transfer events should use the standard Soroban event stream.

## 3. Whether and how to prune

### 3.1 Why the contract cannot delete entries

The contract has no delete path and must not gain one. The append-only
guarantee is what lets a settlement contract trust `get_entry(index)` at
invocation time. Pruning by deletion would break that guarantee.

### 3.2 Current behavior: entries are not TTL-extended

`record` writes persistent entries with `set` and does **not** call
`extend_ttl` on them. Neither does any other code path in the crate. Instance
storage (`Admin`, `EntryCount`) is not extended either. This differs from
`denylist-gate`, which extends each persistent entry explicitly (see
`add_to_denylist`).

Consequences for issuers:

- Each entry lives only for its initial TTL. When that lapses, the entry is
  **archived**, not deleted. The history is recoverable with a
  `RestoreFootprint` operation, but it is not readable by normal reads.
- `get_entry(index)` returns `None` for an archived entry. That is the same
  result as an out-of-range index, so a caller cannot tell "never written"
  from "archived". `list_entries` skips archived entries silently.
- If instance storage archives, `EntryCount` and `Admin` go with it. `record`
  then fails with `NotInitialized` until the instance is restored. Restore it
  before writing, so no entries are lost or misindexed.

This is the gap between the design ("immutable trail") and the current
implementation. Treat the on-chain log as a short-lived buffer until it is
fixed. Mirror entries off-chain as described in section 4.

### 3.3 Options

| Option | What it means | Trade-off |
|---|---|---|
| **A. Let entries archive** (current default) | No extension. Entries archive when their TTL lapses. | Lowest cost. Breaks the on-chain immutability promise and makes `get_entry` ambiguous. Only safe if the off-chain mirror is complete and verified. |
| **B. Extend TTL on every write** | Add `extend_ttl(Entry(index), THRESHOLD, MAX_TTL)` in `record`, like `denylist-gate`. | Entries stay live. Rent grows linearly with the log forever. Costs more per `record` call. Matches the stated design. |
| **C. Rotate the log** | Deploy a new `audit-log` instance at a fixed interval (e.g. yearly) and point primitives at it with `set_audit_log`. | Bounds each instance's size. The old instance stays readable by contract ID until it is archived. Callers must know which instance covers which period. |
| **D. Delete entries** | Not supported. | Breaks Invariant 4.3.2. Do not do this. |

**Recommendation:** Adopt **B** for entries that must remain on-chain for
proof-of-audit, and combine it with **C** to keep any single instance from
growing without bound. Until the contract changes, **A** is acceptable only if
the mirror in section 4 is running and reconciled. Changing the contract is out
of scope for this document. Track it as a separate change to
`contracts/audit-log`, with its own tests and storage-versioning note (see
`STORAGE_VERSIONING.md`).

### 3.4 Retention period

How long you keep entries is a regulatory question, not a technical one. This
document does not set a retention period. Set it from the record-keeping rules
in each jurisdiction where you operate, and make sure the off-chain copies
(section 4) meet that period, not just the on-chain entries.

## 4. Mirroring entries with `tools/indexer`

The indexer is the only supported way to keep an off-chain copy of the log. It
writes one row per `ComplianceEvent` into the `audit_log` table
(`tools/indexer/src/db.ts`).

### 4.1 Setup

Set `AUDIT_LOG_CONTRACT_ID` in `tools/indexer/.env` to your deployed
`audit-log` contract ID. Set `START_LEDGER` to the ledger where the log was
deployed, or where you want the mirror to begin. Run the indexer continuously
with `npm run dev` or the Docker image, and keep the SQLite file on a volume
that is backed up.

### 4.2 Timing: the RPC retention window is the real deadline

`getEvents` on Soroban RPC only returns events inside the node's retention
window. Events older than that cannot be fetched again. The indexer's own
comment cites about 7 days on testnet. Its fallback start point uses
`17,280` ledgers (about 24 hours at 5 seconds per ledger), which is much
shorter. Check your RPC provider's actual retention, and do not rely on the
fallback.

Poll often enough that no event ages out before it is ingested. Also make sure
the mirror is caught up before any on-chain entry could be archived (section
3.2).

### 4.3 Detecting gaps

The indexer logs `Ledger gap detected` when `getEvents` reports a horizon
earlier than the requested ledger, and `Ledger regression` or `gap` when the
poll start does not match the checkpoint in `indexer_state`. Treat either
message as an incident. Restart with a `START_LEDGER` that covers the missing
range, and confirm the table is complete afterward (see 4.4).

### 4.4 Reconciling the mirror against the chain

The audit log's counter is the source of truth for how many entries exist.
Compare the mirror's row count with it periodically:

```sql
SELECT COUNT(*) FROM audit_log WHERE contract_id = '<AUDIT_LOG_CONTRACT_ID>';
```

Compare this with `entry_count()` from the contract. They should match once the
indexer has processed through the latest ledger. The indexer skips a
`ComplianceEvent` silently when a field is missing (`e.kind`, `e.address`, or
`e.source`), so a mismatch can also mean dropped events. Investigate any
difference before it grows, because archived entries cannot be recovered from
the chain once their restore window passes.

### 4.5 Exporting for long-term retention

The indexer database is a standard SQLite file. For long-term retention:

1. Stop or pause writes long enough to take a consistent copy, or copy the
   file between poll cycles.
2. Export the table to an immutable store such as object storage with
   versioning or write-once retention, under the period set in section 3.4.
3. Record the last ledger in the export (`indexer_state`, key `last_ledger`)
   so later exports can continue from it.
4. Verify the export by reloading it and repeating the count check in 4.4.

Do not treat the SQLite file as the only copy.

## 5. Open items

- Entries have no TTL extension (section 3.2). Decide between options B and C
  in section 3.3 and implement it in `contracts/audit-log`.
- `get_entry` and `list_entries` cannot distinguish archived entries from
  missing ones. Document this for callers, or expose a status in the contract.
- The indexer's fallback start (`17,280` ledgers) and its doc comment (about 7
  days) disagree. Align them with the real RPC retention window.
