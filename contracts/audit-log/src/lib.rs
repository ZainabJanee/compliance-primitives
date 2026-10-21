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
//! the stored audit-log address, so the cost and code path are zero when
//! the feature is not configured.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype, Address,
    Env, String, Symbol, Vec,
};

/// Maximum number of entries `list_entries` will return in a single call.
/// Bounds the resources consumed by a single invocation regardless of how
/// large `limit` is requested.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Maximum byte length of the `detail` string passed to `record`.
///
/// Keeping entries small limits per-entry storage cost and prevents a caller
/// from hitting Soroban's per-entry size cap mid-integration. Callers should
/// check against this constant before constructing a `record` call.
pub const MAX_DETAIL_LEN: u32 = 256;

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
    /// Free-form detail string for additional context. Length is capped at
    /// [`MAX_DETAIL_LEN`] bytes to limit per-entry storage cost.
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
    /// Contract is paused; no writes are accepted.
    ContractPaused = 5,
    /// `detail` string passed to `record` exceeds [`MAX_DETAIL_LEN`] bytes.
    DetailTooLong = 6,
}

// ---------------------------------------------------------------------------
// Cross-contract client trait (for callers of this contract)
// ---------------------------------------------------------------------------

/// Trait that generates `AuditLogClient` via `#[contractclient]`. Other
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
        env.events().publish((), soroban_sdk::symbol_short!("Paused"));
        Ok(())
    }

    /// Resume write operations after a pause. Admin-only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        compliance_pausable::unpause(&env);
        env.events().publish((), soroban_sdk::symbol_short!("Unpaused"));
        Ok(())
    }

    /// Check if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        compliance_pausable::is_paused(&env)
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

        // Reject oversized detail strings to cap per-entry storage cost.
        if detail.len() > MAX_DETAIL_LEN {
            return Err(Error::DetailTooLong);
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

        env.storage()
            .persistent()
            .set(&DataKey::Entry(index), &entry);

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
}

#[cfg(test)]
mod test;
