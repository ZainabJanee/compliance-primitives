//! `audit-log` is a `#![no_std]` Soroban contract that provides an
//! **on-chain, append-only audit trail** for compliance events emitted by
//! other primitives in this workspace (and any caller that integrates it).
//!
//! ## Why on-chain instead of off-chain indexing?
//!
//! Off-chain indexers (Horizon, Mercury, custom event pipelines) are the
//! right tool for analytics and dashboards, but they have a gap: another
//! contract cannot read their state at transaction time. An on-chain audit
//! log fills that gap:
//!
//! - **Single queryable address** — any contract or client that knows the
//!   log's contract ID can call `get_entry(index)` or `entry_count()` at
//!   invocation time, enabling on-chain proof-of-audit patterns (e.g. a
//!   settlement contract that verifies a prior compliance event exists).
//! - **Immutable trail** — entries are written with persistent storage and
//!   never deleted; the append-only counter guarantees ordering.
//! - **No indexer dependency** — the log is readable even before an indexer
//!   has caught up, and remains correct if the indexer is unavailable.
//! - **Tradeoff** — every `record` call costs ledger storage (one
//!   persistent entry per event). This is appropriate for lower-frequency
//!   compliance events (denylist mutations, jurisdiction overrides) rather
//!   than high-throughput transfer events; for the latter, rely on the
//!   standard Soroban event stream.
//!
//! ## Write volume and storage growth
//!
//! Each `record` call does two storage writes: one new persistent entry at
//! `DataKey::Entry(index)` and an update to the `EntryCount` counter in
//! instance storage. Both are unmeasured estimates here; re-measure with the
//! method in `BENCHMARKS.md` before relying on any number.
//!
//! - **Per-entry footprint** — roughly 150–300 bytes for typical entries
//!   (two `Address` values, a `Symbol`, a `detail` string of up to a few
//!   dozen bytes, and a `u32` ledger). Rent scales with bytes × retained
//!   ledgers, so total storage cost grows linearly with event count and
//!   retention period.
//! - **Throughput ceiling** — every `record` on one instance reads and
//!   writes the same `EntryCount` key, so calls serialize. A single audit
//!   log cannot absorb more than roughly one `record` per ledger (about one
//!   every ~5 seconds), and bursts of concurrent submissions should be
//!   expected to fail and need retry.
//! - **Volume guidance** (indicative, not benchmarked):
//!   - up to ~1,000 events/day: direct on-chain logging is comfortable;
//!   - ~1,000–10,000 events/day: viable, but budget for rent and retry
//!     handling, and keep `detail` short;
//!   - above ~10,000 events/day: keep the on-chain log for the events that
//!     must be provable on-chain, rely on the Soroban event stream and an
//!     off-chain indexer for the rest, and consider committing a periodic
//!     hash or Merkle root of a batch on-chain instead of every event.
//! - **TTL** — `record` extends each new entry's TTL (see "Storage and TTL
//!   policy" below), but entries that are never touched again still expire
//!   once that window lapses. Issuers relying on retention beyond ~90 days
//!   must run an external restore/extend process; until then, do not treat
//!   `get_entry` as a durable record past that window.
//!
//! ## Callers
//!
//! - **Writers (`record`)** — contracts registered as an audit-log source.
//!   The caller passes its own address as `source`, and that address must
//!   authorize the invocation (`source.require_auth()`), so one primitive
//!   cannot write entries attributed to another. The ledger sequence is
//!   stamped on each entry by the log itself.
//! - **Admin (`initialize`, `pause`, `unpause`)** — the address passed to
//!   `initialize` (exactly once) can pause and resume writes. The admin is
//!   *not* checked on `record`: any authorized `source` may write while the
//!   log is unpaused.
//! - **Readers (`get_entry`, `entry_count`, `list_entries`, `is_paused`)** —
//!   unauthenticated. Any contract, client, or script can read. `list_entries`
//!   returns at most [`MAX_PAGE_SIZE`] entries per call and fails with
//!   `PageTooLarge` above that, so larger reads page through `start`.
//! - **Indexers** — subscribe to the `ComplianceEvent` contract event
//!   (topics: `kind`, `subject`) rather than polling storage.
//!
//! ## Composition with the other primitives
//!
//! `audit-log` has no dependency on the other compliance contracts; its only
//! workspace dependency is the shared `compliance-pausable` crate, which
//! provides the pause storage and the write guard used by `pause`,
//! `unpause`, and `record`.
//!
//! - **`denylist-gate`** is the reference integration. It depends on this
//!   crate and exposes `set_audit_log(admin, audit_log_id)`. Once set, each
//!   denylist add and remove calls `record` through the generated
//!   `AuditLogClient`, with `deny_add` / `deny_remove` as the kind and the
//!   denylisted address as the subject.
//! - **`AuditLogInterface`** (below) is the trait to copy or depend on when a
//!   caller wants to invoke `record` without linking this contract's binary.
//! - Other primitives do not write to the log today. Wiring one in follows
//!   the same opt-in pattern.
//!
//! ## Opt-in integration pattern
//!
//! This contract is **not** called automatically by the other primitives.
//! A deployer wires it in by:
//!
//! 1. Deploying an `audit-log` instance and noting its contract ID.
//! 2. Calling `set_audit_log(admin, audit_log_id)` on the primitive (e.g.
//!    `denylist-gate`) to register the log address.
//! 3. From that point on, every state-mutating call on the primitive will
//!    additionally call `audit-log.record(...)` via cross-contract
//!    invocation — the primitive acts as the `source` address in the entry.
//!
//! If `set_audit_log` is never called, the primitive behaves exactly as
//! before: the audit log path is guarded by an `if let Some(...)` check on
//! the stored audit-log address, so it adds no cross-contract call when the
//! feature is not configured.
//!
//! ## Storage and TTL policy
//!
//! Each log entry lives in **persistent** storage under its own key. Every
//! write path (`record`) extends that entry's TTL: whenever the remaining TTL
//! has dropped below [`ENTRY_TTL_THRESHOLD`] (~7 days) it is bumped back up to
//! [`ENTRY_TTL_EXTEND_TO`] (~90 days). This keeps recorded events from being
//! archived by Soroban state expiration — an archived entry would make
//! `get_entry` and `list_entries` return nothing until a manual restore.
//!
//! ## Upgradeability
//!
//! `upgrade(admin, new_wasm_hash)` lets the configured admin move the
//! contract's code to a new WASM hash via
//! `env.deployer().update_current_contract_wasm`, following the same
//! admin-gated pattern used by `compliance-aggregator` and `jurisdiction-flag`.
//! Only the stored admin may call it; any other caller is rejected with
//! `Error::NotAuthorized`, and calling it before `initialize` returns
//! `Error::NotInitialized`. An `UpgradePerformed` event is emitted so the
//! upgrade is visible to indexers.
//!
//! The WASM swap does not touch storage: the admin key and the entry counter
//! live in instance storage, and every `Entry(index)` lives in persistent
//! storage under the same `DataKey` variants, so the log is intact and
//! callable immediately after the upgrade. See `UPGRADE.md` in this crate for
//! the full procedure and the storage rules a new version must follow.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype, Address,
    BytesN, Env, String, Symbol, Vec,
};

/// Maximum number of entries `list_entries` will return in a single call.
/// Bounds the resources consumed by a single invocation regardless of how
/// large `limit` is requested.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Extend a persistent log entry when its remaining TTL drops below this many
/// ledgers (~7 days at ~5s/ledger).
pub const ENTRY_TTL_THRESHOLD: u32 = 120_960;
/// Target remaining TTL, in ledgers, after an entry is extended (~90 days at
/// ~5s/ledger).
pub const ENTRY_TTL_EXTEND_TO: u32 = 1_555_200;

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// A single compliance event stored on-chain.
#[contracttype]
#[derive(Clone)]
pub struct LogEntry {
    /// The contract (or EOA) that called `record` — typically one of the
    /// compliance-primitive contracts acting as the recording source.
    pub source: Address,
    /// A short, human-readable event kind, e.g. `"deny_add"`, `"deny_remove"`,
    /// `"jurisdiction_set"`. Uses `Symbol` so it is compact on-chain.
    pub kind: Symbol,
    /// The address the event is *about* (the address being denied, flagged, etc.).
    pub subject: Address,
    /// Free-form detail string for additional context (kept short to limit storage cost).
    pub detail: String,
    /// The ledger sequence number at the time of recording.
    pub ledger: u32,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address allowed to call `initialize`.
    Admin,
    /// Running count of entries; used as the next index.
    EntryCount,
    /// Individual log entry keyed by its zero-based index.
    Entry(u64),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted on every successful `record` call so off-chain indexers can track
/// events without having to read persistent storage.
#[contractevent]
pub struct ComplianceEvent {
    #[topic]
    pub kind: Symbol,
    #[topic]
    pub subject: Address,
    pub source: Address,
    pub detail: String,
}

/// Emitted whenever the contract is upgraded to a new WASM implementation,
/// for on-chain auditability of the upgrade path.
#[contractevent]
pub struct UpgradePerformed {
    #[topic]
    pub admin: Address,
    pub new_wasm_hash: BytesN<32>,
}

#[contractevent]
pub struct Paused {
    #[topic]
    pub admin: Address,
}

#[contractevent]
pub struct Unpaused {
    #[topic]
    pub admin: Address,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    /// `limit` passed to `list_entries` exceeds [`MAX_PAGE_SIZE`].
    PageTooLarge = 4,
    /// `record` was called while the contract is paused.
    ContractPaused = 5,
}

// ---------------------------------------------------------------------------
// Cross-contract client trait (for callers of this contract)
// ---------------------------------------------------------------------------

/// Trait that generates `AuditLogInterfaceClient` via `#[contractclient]`. Other
/// contracts that want to cross-call `audit-log.record(...)` should define
/// this same trait locally (or depend on this crate's `rlib`) and use the
/// generated client rather than linking the full contract binary.
#[contractclient(name = "AuditLogInterfaceClient")]
pub trait AuditLogInterface {
    fn record(
        env: Env,
        source: Address,
        kind: Symbol,
        subject: Address,
        detail: String,
    );
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct AuditLog;

#[contractimpl]
impl AuditLog {
    /// One-time setup. `admin` is stored as the contract owner.
    ///
    /// The admin is not currently used for access control on `record` (any
    /// authenticated `source` may record); it is stored for future
    /// governance operations (e.g. migrating the log or pausing it).
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        // Seed the counter at zero.
        env.storage()
            .instance()
            .set(&DataKey::EntryCount, &0u64);
        Ok(())
    }

    /// Pause all write operations. Admin-only.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        compliance_pausable::pause(&env);
        Paused {
            admin: admin.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Resume write operations after a pause. Admin-only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        compliance_pausable::unpause(&env);
        Unpaused {
            admin: admin.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Check if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        compliance_pausable::is_paused(&env)
    }

    /// Upgrade the contract WASM. Admin-only.
    ///
    /// Swaps the code behind this contract ID to `new_wasm_hash` (which must
    /// already be uploaded to the network). The admin key, the entry counter
    /// and every recorded entry are preserved across the upgrade. See the
    /// module-level "Upgradeability" section for the full procedure.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        UpgradePerformed {
            admin,
            new_wasm_hash: new_wasm_hash.clone(),
        }
        .publish(&env);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Append a compliance event to the log.
    ///
    /// `source` must authorize this call (i.e. the calling contract must
    /// pass its own address as `source` and Soroban's auth framework will
    /// verify the invocation). This ensures entries cannot be forged by an
    /// address other than the one claiming to be the source.
    pub fn record(
        env: Env,
        source: Address,
        kind: Symbol,
        subject: Address,
        detail: String,
    ) -> Result<(), Error> {
        compliance_pausable::require_not_paused_or(&env, Error::ContractPaused)?;

        // Contract must be initialized before accepting entries.
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

        // The source must authorize this invocation.
        source.require_auth();

        let index: u64 = env
            .storage()
            .instance()
            .get(&DataKey::EntryCount)
            .unwrap_or(0u64);

        let entry = LogEntry {
            source: source.clone(),
            kind: kind.clone(),
            subject: subject.clone(),
            detail: detail.clone(),
            ledger: env.ledger().sequence(),
        };

        let entry_key = DataKey::Entry(index);
        env.storage().persistent().set(&entry_key, &entry);
        env.storage()
            .persistent()
            .extend_ttl(&entry_key, ENTRY_TTL_THRESHOLD, ENTRY_TTL_EXTEND_TO);

        env.storage()
            .instance()
            .set(&DataKey::EntryCount, &(index + 1));

        ComplianceEvent {
            kind,
            subject,
            source,
            detail,
        }
        .publish(&env);

        Ok(())
    }

    /// Return the log entry at `index`, or `None` if the index is out of
    /// range.
    pub fn get_entry(env: Env, index: u64) -> Option<LogEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::Entry(index))
    }

    /// Return up to `count` log entries starting at zero-based index `start`.
    ///
    /// Unlike [`AuditLog::list_entries`], an oversized `count` is clamped to
    /// [`MAX_PAGE_SIZE`] rather than rejected. Fewer entries are returned when
    /// the range runs past `entry_count()`.
    pub fn get_entries(env: Env, start: u64, count: u32) -> Vec<LogEntry> {
        let limit = count.min(MAX_PAGE_SIZE);
        Self::list_entries(env.clone(), start, limit).unwrap_or(Vec::new(&env))
    }

    /// Return the total number of entries recorded so far.
    pub fn entry_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::EntryCount)
            .unwrap_or(0u64)
    }

    /// Return a bounded page of log entries, starting at zero-based index
    /// `start` and containing at most `limit` entries.
    ///
    /// Stops early (returning fewer than `limit` entries) once `start`
    /// reaches the current entry count. `limit` must not exceed
    /// [`MAX_PAGE_SIZE`] — callers that need more entries should page
    /// through with successive calls.
    pub fn list_entries(env: Env, start: u64, limit: u32) -> Result<Vec<LogEntry>, Error> {
        if limit > MAX_PAGE_SIZE {
            return Err(Error::PageTooLarge);
        }

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::EntryCount)
            .unwrap_or(0u64);

        let mut entries = Vec::new(&env);
        let mut index = start;
        let mut collected: u32 = 0;
        while collected < limit && index < count {
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<DataKey, LogEntry>(&DataKey::Entry(index))
            {
                entries.push_back(entry);
            }
            index += 1;
            collected += 1;
        }

        Ok(entries)
    }

    /// Require `admin` to authorize the call and to match the stored admin.
    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if stored_admin != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
