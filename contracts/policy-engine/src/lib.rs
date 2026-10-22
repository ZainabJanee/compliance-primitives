//! `policy-engine` is a `#![no_std]` Soroban contract that composes multiple
//! compliance checks into a single policy evaluation call.
//!
//! ## Purpose
//!
//! Each compliance primitive in this workspace answers one narrow question
//! about an address ("is it denylisted?", "is it in a permitted
//! jurisdiction?", "is it allowlisted?"). Real issuance policies are rarely
//! that narrow: they combine several of those answers with AND / OR logic
//! and must be changeable as regulation evolves. `policy-engine` holds that
//! composition on-chain, so a token contract makes **one** call —
//! `evaluate(from, to)` — and gets back a single pass/fail decision for the
//! whole policy, instead of hard-coding a sequence of cross-contract calls
//! that would need a redeploy every time the policy changes.
//!
//! ## Who calls it
//!
//! - **Token / transfer contracts** (the primary callers) invoke
//!   `evaluate(from, to)` or `batch_evaluate(pairs)` before moving value and
//!   refuse the transfer when the result is `false`. These calls are
//!   permissionless and require no auth.
//! - **The policy admin** (an issuer key or, preferably, a `multisig-admin`
//!   contract address) calls `initialize`, `add_check`, `remove_check` and
//!   `set_circuit_breaker` to manage the policy. Every mutation requires the
//!   stored admin's auth.
//! - **Off-chain tooling and auditors** call the read-only views
//!   (`get_checks`, `get_op`, `get_policy`, `circuit_breaker`) and consume
//!   the `PolicyResult` event stream to reconstruct every decision.
//!
//! ## How it composes with the other contracts
//!
//! | Contract | Relationship to `policy-engine` |
//! |---|---|
//! | `denylist-gate` | Registered as a [`CheckKind::Denylist`] check; `evaluate` calls its `check(address)`. |
//! | `jurisdiction-flag` | Registered as a [`CheckKind::Jurisdiction`] check; `evaluate` calls `is_permitted_jurisdiction(address, allowed_codes)`. |
//! | `allowlist-token` | Registered as a [`CheckKind::Allowlist`] check; `evaluate` calls its `is_allowed(address)`. |
//! | `circuit-breaker` | Optional emergency stop. When configured and frozen, `evaluate` denies outright without running any check. |
//! | `multisig-admin` | Recommended value for the `admin` address, so policy changes require M-of-N approval via `__check_auth`. |
//! | `pausable` | Shared crate used to guard admin mutations (`add_check`, `remove_check`) while the contract is paused. |
//! | `audit-log` | Complementary: integrators that need an on-chain, contract-readable trail of policy changes record them there; `policy-engine` itself only emits events. |
//! | `compliance-aggregator` | Sibling, not a dependency. The aggregator is a fixed AND-only fan-out optimised for call overhead; `policy-engine` is the configurable AND / OR layer. Pick one per integration. |
//!
//! The primitives are called through locally declared `#[contractclient]`
//! interfaces rather than direct crate dependencies, so any contract that
//! exposes the same function signature can be plugged in as a check.
//!
//! ## AND / OR design choice
//!
//! Compliance use cases almost always reduce to one of two logical shapes:
//! every check must pass (AND — "this address must be allowlisted **and** in
//! a permitted jurisdiction") or at least one must pass (OR — "this address
//! either cleared KYC with provider A **or** provider B"). A two-variant
//! `CombineOp` enum (`All` / `Any`) covers both without the overhead of an
//! AST, which would be over-engineered for this domain and would add
//! significant complexity to the storage, serialization, and auditing story.
//!
//! ## Check failure surfacing
//!
//! `evaluate` returns `Ok(bool)` rather than panicking or returning an
//! opaque error on a failed policy: a Soroban invocation that returns a
//! contract error rolls back all state changes including emitted events, so
//! any failure audit trail would be silently discarded. By returning
//! `Ok(false)` and emitting a `PolicyResult` event the caller gets a
//! machine-readable result and the chain preserves an auditable record of
//! the decision, consistent with the pattern used in `allowlist-token`.
//!
//! ## Registration design
//!
//! Checks are stored as an admin-managed, mutable `Vec<CheckKind>` in
//! persistent storage. This lets the issuer add, reorder, or remove checks
//! as compliance requirements evolve — without redeploying or upgrading the
//! contract binary. An authorized admin is required for all mutations,
//! keeping the policy immutable to unprivileged callers.
//!
//! ## Upgradeability
//!
//! The admin can propose an upgrade with `propose_upgrade(admin, new_wasm,
//! delay_ledgers)`. It can only be installed with `commit_upgrade` after the
//! delay expires, and the admin can call `cancel_upgrade` during that window.
//! The WASM swap preserves contract storage, including the admin, policy, and
//! checks. A successful commit emits `UpgradePerformed`.
//!
//! ## Storage and TTL policy
//!
//! All state (admin, `CombineOp`, the `Checks` vec and the optional
//! circuit-breaker address) lives in **instance** storage, which shares a
//! single TTL with the contract instance itself. Every write path
//! (`initialize`, `set_circuit_breaker`, `add_check`, `remove_check`,
//! `propose_upgrade`, `cancel_upgrade`)
//! finishes by extending that TTL: whenever the remaining TTL has dropped
//! below [`INSTANCE_TTL_THRESHOLD`] (~7 days) it is bumped back up to
//! [`INSTANCE_TTL_EXTEND_TO`] (~90 days). This keeps the policy from being
//! archived by Soroban state expiration — an archived policy would make
//! `evaluate` unreachable until a manual restore.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    Address, BytesN, Env, String, Symbol, Vec,
};

// ---------------------------------------------------------------------------
// Cross-contract client interfaces
// ---------------------------------------------------------------------------

/// Describes the `denylist-gate` contract interface used for cross-contract
/// calls. The generated `DenylistCheckClient` is used in `evaluate` to call
/// `check()` on a deployed denylist-gate instance. We do not take a direct
/// crate dependency on `denylist-gate` to avoid colliding wasm exports.
#[contractclient(name = "DenylistCheckClient")]
pub trait DenylistCheckInterface {
    fn check(env: Env, address: Address) -> bool;
}

/// Describes the `jurisdiction-flag` contract interface used for
/// cross-contract calls. The generated `JurisdictionCheckClient` is used in
/// `evaluate`. Same reason as above for avoiding a direct crate dep.
#[contractclient(name = "JurisdictionCheckClient")]
pub trait JurisdictionCheckInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

/// Describes the `circuit-breaker` contract interface used for cross-contract
/// calls. Same reason as above for avoiding a direct crate dep.
#[contractclient(name = "CircuitBreakerClient")]
pub trait CircuitBreakerInterface {
    fn is_frozen(env: Env) -> bool;
}

/// Describes the `allowlist-token` contract interface used for the policy
/// engine's allowlist check.
#[contractclient(name = "AllowlistCheckClient")]
pub trait AllowlistCheckInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// Parameters for a denylist-gate check.
#[contracttype]
#[derive(Clone)]
pub struct DenylistCheck {
    /// Address of the deployed `denylist-gate` contract to call.
    pub contract: Address,
}

/// Parameters for a jurisdiction-flag check.
#[contracttype]
#[derive(Clone)]
pub struct JurisdictionCheck {
    /// Address of the deployed `jurisdiction-flag` contract to call.
    pub contract: Address,
    /// The set of jurisdiction codes that are permitted.
    pub allowed_codes: Vec<String>,
}

/// Parameters for an allowlist-token check.
#[contracttype]
#[derive(Clone)]
pub struct AllowlistCheck {
    /// Address of the deployed `allowlist-token` contract to call.
    pub contract: Address,
}

/// Parameters for a circuit-breaker check.
///
/// When this check is evaluated, the engine calls `is_frozen()` on the
/// configured circuit-breaker contract. The check **passes** only when the
/// circuit-breaker is **not** frozen (i.e. the emergency stop has not been
/// triggered). This lets an issuer compose an explicit "is the system
/// live?" gate alongside denylist and jurisdiction rules inside a single
/// policy tree, rather than relying solely on the top-level circuit-breaker
/// short-circuit in `evaluate`.
#[contracttype]
#[derive(Clone)]
pub struct CircuitBreakerCheck {
    /// Address of the deployed `circuit-breaker` contract to call.
    pub contract: Address,
}

/// Describes a single compliance check the engine should perform.
///
/// Each variant carries the address of the external contract that implements
/// the check plus any parameters that check needs.
///
/// ## `#[contracttype]` enum variant shape restriction
///
/// The Soroban SDK's `#[contracttype]` macro (pinned in this workspace) only
/// supports **unit** or **single-tuple** enum variants. Named-field variants
/// such as:
///
/// ```ignore
/// // ❌ Does NOT compile with #[contracttype]
/// pub enum CheckKind {
///     Jurisdiction { contract: Address, allowed_codes: Vec<String> },
/// }
/// ```
///
/// are rejected by the macro with a compile error. This is a known limitation
/// of the XDR-based storage encoding used by Soroban: each variant is stored
/// as a tagged union, and named fields would require an extra layer of
/// encoding that the macro does not generate.
///
/// **The correct pattern** is to wrap multi-field parameters in a dedicated
/// `#[contracttype]` struct and use a single-tuple variant:
///
/// ```ignore
/// // ✅ Correct: wrap multi-field params in a struct
/// #[contracttype]
/// pub struct JurisdictionCheck {
///     pub contract: Address,
///     pub allowed_codes: Vec<String>,
/// }
///
/// #[contracttype]
/// pub enum CheckKind {
///     Jurisdiction(JurisdictionCheck),  // single-tuple variant — compiles
/// }
/// ```
///
/// If you add a new `CheckKind` variant that needs multiple parameters, follow
/// this same pattern: define a `#[contracttype]` struct for the parameters,
/// then add a single-tuple variant wrapping that struct. **Do not** attempt to
/// use named-field variants, even if a future contributor believes the SDK now
/// supports them — verify against the pinned SDK version in `Cargo.toml`
/// before changing this shape.
#[contracttype]
#[derive(Clone)]
pub enum CheckKind {
    /// Call `denylist-gate.check(address)`. The address must **not** be on
    /// the denylist for this check to pass.
    Denylist(DenylistCheck),
    /// Call `jurisdiction-flag.is_permitted_jurisdiction(address,
    /// allowed_codes)`. The address must have a jurisdiction code in
    /// `allowed_codes` for this check to pass.
    Jurisdiction(JurisdictionCheck),
    /// Call `allowlist-token.is_allowed(address)`. The address must be
    /// present on the allowlist for this check to pass.
    Allowlist(AllowlistCheck),
    /// Call `circuit-breaker.is_frozen()`. The check passes only when the
    /// circuit-breaker is **not** frozen. Use this to embed an emergency-stop
    /// gate directly in the policy tree so a single call to `evaluate`
    /// covers the full compliance stack including the freeze check.
    CircuitBreaker(CircuitBreakerCheck),
}

/// How the engine combines the results of multiple checks.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CombineOp {
    /// AND — every check must pass. Used when an address must satisfy all
    /// compliance requirements simultaneously.
    All,
    /// OR — at least one check must pass. Used when multiple equivalent
    /// compliance paths exist (e.g. two KYC providers).
    Any,
}

/// Describes a pair of addresses (e.g. sender and recipient) for evaluating
/// compliance policies on transfers in batches.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddressPair {
    pub from: Address,
    pub to: Address,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Checks,
    CombineOp,
    /// Address of the `circuit-breaker` contract to consult, if any. When
    /// set and frozen, `evaluate` short-circuits to deny.
    CircuitBreaker,
    PendingUpgrade,
}

/// Delayed upgrade proposal state.
#[contracttype]
#[derive(Clone)]
pub struct UpgradeState {
    pub new_wasm: BytesN<32>,
    pub activated_at: u64,
}

// ---------------------------------------------------------------------------
// Errors and events
// ---------------------------------------------------------------------------

/// Maximum number of addresses accepted by `batch_evaluate` in a single call.
///
/// Soroban imposes a per-transaction CPU-instruction and memory budget. Each
/// address in the batch requires one or more cross-contract calls (one per
/// registered check), so an unbounded list would allow a caller to exhaust
/// the budget and brick the transaction. 20 was chosen to mirror the limit
/// used in the `compliance-aggregator` batch family and to keep the worst-case
/// instruction cost within the conservative end of the Soroban default budget.
pub const MAX_BATCH_SIZE: u32 = 20;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    PolicyViolation = 4,
    /// Returned by `add_check` when the number of registered checks would
    /// exceed `MAX_CHECKS`. Keeps per-evaluation resource cost bounded and
    /// prevents unbounded storage growth.
    MaxDepthExceeded = 5,
    /// Returned by `swap_checks` when either index `i` or `j` is out of
    /// range for the current check list.
    IndexOutOfBounds = 6,
    /// Returned by state-changing entry points while the contract is paused.
    ContractPaused = 7,
    /// Returned by `get_check` when the requested index is out of range.
    CheckIndexOutOfRange = 8,
    /// Returned by `commit_upgrade` when no upgrade is pending or its delay
    /// has not elapsed.
    UpgradeNotReady = 9,
}

/// Maximum number of checks that can be registered in a single policy
/// engine instance.  Chosen to keep per-`evaluate` cross-contract call
/// overhead well within Soroban's instruction limits while still supporting
/// all realistic compliance stack sizes.
pub const MAX_CHECKS: u32 = 16;

/// Extend the contract's instance storage when its remaining TTL drops below
/// this many ledgers (~7 days at ~5s/ledger).
pub const INSTANCE_TTL_THRESHOLD: u32 = 120_960;

/// Target remaining TTL, in ledgers, after an extension (~90 days at
/// ~5s/ledger).
pub const INSTANCE_TTL_EXTEND_TO: u32 = 1_555_200;

/// Emitted by `evaluate` regardless of the pass/fail outcome so that
/// off-chain compliance tooling can build a full audit trail even when the
/// policy passes (which wouldn't produce an error event).
#[contractevent]
pub struct PolicyResult {
    #[topic]
    pub passed: bool,
    pub from: Address,
    pub to: Address,
}

/// Emitted whenever the contract is upgraded to a new WASM implementation,
/// for on-chain auditability of the upgrade path.
#[contractevent]
pub struct UpgradePerformed {
    #[topic]
    pub admin: Address,
}

/// Carries information about which check in the list failed and what kind it
/// was. Serializable on-chain, so it can be embedded in contract events or
/// returned from view calls.
///
/// ## Intended use
///
/// `CheckFailure` is the payload surfaced by [`PolicyEngine::evaluate_verbose`],
/// an extended variant of `evaluate` that returns the index and kind of the
/// **first** check that caused the policy to fail, in addition to the overall
/// pass/fail result. This makes it possible to diagnose a failed policy
/// evaluation without re-running each check individually in a separate call.
///
/// ### Design sketch
///
/// ```text
/// evaluate_verbose(env, from, to)
///   -> Result<(bool, Option<CheckFailure>), Error>
/// ```
///
/// - When the policy **passes**, the second element is `None`.
/// - When the policy **fails**, the second element is `Some(CheckFailure)`
///   where `check_index` is the zero-based position of the first failing check
///   in the registered `Vec<CheckKind>` and `kind` is a `Symbol` naming the
///   variant (e.g. `Symbol::new(env, "Denylist")`).
///
/// Off-chain tooling (block explorers, compliance dashboards) can call
/// `evaluate_verbose` once and immediately know both the outcome and, on
/// failure, precisely which rule triggered the rejection — without needing to
/// reconstruct the full check list from storage.
///
/// ### Why `Option<CheckFailure>` instead of embedding in the error
///
/// `evaluate` deliberately returns `Ok(false)` on a policy failure rather than
/// `Err(...)` so that the emitted `PolicyResult` event is not rolled back (see
/// the module-level doc comment for the full rationale). `evaluate_verbose`
/// preserves this invariant: the `CheckFailure` detail rides in the `Ok(…)`
/// payload alongside the bool, keeping events auditable while surfacing richer
/// diagnostic data to callers.
///
/// ### Tracking
///
/// `evaluate_verbose` is implemented in this contract. See the
/// `evaluate_verbose` function below. Future work could extend `CheckFailure`
/// with additional fields (e.g. the address that failed, or the full
/// `CheckKind` payload) if callers need more context.
#[contracttype]
#[derive(Clone)]
pub struct CheckFailure {
    /// Zero-based index into the `Vec<CheckKind>` of the first failing check.
    pub check_index: u32,
    /// Short `Symbol` name of the `CheckKind` variant that failed, e.g.
    /// `"Denylist"`, `"Jurisdiction"`, `"Allowlist"`, or `"CircuitBreaker"`.
    pub kind: Symbol,
}

/// A snapshot of the full policy configuration: the combination operator and
/// the ordered list of registered checks.
///
/// Returned by [`PolicyEngine::get_policy`] so off-chain tooling and auditors
/// can read back the complete policy in a single view call.
#[contracttype]
#[derive(Clone)]
pub struct PolicyNode {
    /// How check results are combined (`All` = AND, `Any` = OR).
    pub op: CombineOp,
    /// The ordered list of checks that `evaluate` will run.
    pub checks: Vec<CheckKind>,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct PolicyEngine;

#[contractimpl]
impl PolicyEngine {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// One-time initializer. Sets `admin` as the authorized manager and
    /// `op` as the combination operator for all future evaluations.
    /// Initializes the check list to empty.
    pub fn initialize(
        env: Env,
        admin: Address,
        op: CombineOp,
        circuit_breaker: Option<Address>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::CombineOp, &op);
        let empty: Vec<CheckKind> = Vec::new(&env);
        env.storage().instance().set(&DataKey::Checks, &empty);
        if let Some(breaker) = circuit_breaker {
            env.storage()
                .instance()
                .set(&DataKey::CircuitBreaker, &breaker);
        }
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Register or replace the `circuit-breaker` contract address.
    /// Admin-only. Pass this to enable emergency-freeze short-circuiting.
    pub fn set_circuit_breaker(env: Env, admin: Address, breaker: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::CircuitBreaker, &breaker);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Returns the currently registered `circuit-breaker` address, if any.
    pub fn circuit_breaker(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::CircuitBreaker)
    }

    /// Propose an upgrade to an uploaded Wasm hash. The admin may cancel it
    /// before `delay_ledgers` have elapsed.
    pub fn propose_upgrade(
        env: Env,
        admin: Address,
        new_wasm: BytesN<32>,
        delay_ledgers: u32,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let state = UpgradeState {
            new_wasm,
            activated_at: u64::from(env.ledger().sequence())
                .saturating_add(delay_ledgers as u64),
        };
        env.storage().instance().set(&DataKey::PendingUpgrade, &state);
        Self::extend_instance_ttl(&env);
        env.events()
            .publish((soroban_sdk::symbol_short!("upgprop"),), (admin, delay_ledgers));
        Ok(())
    }

    /// Commit a pending upgrade after its delay has elapsed.
    pub fn commit_upgrade(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let state: UpgradeState = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(Error::UpgradeNotReady)?;
        if u64::from(env.ledger().sequence()) < state.activated_at {
            return Err(Error::UpgradeNotReady);
        }
        env.deployer().update_current_contract_wasm(state.new_wasm);
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        UpgradePerformed { admin }.publish(&env);
        Ok(())
    }

    /// Cancel a pending upgrade. Admin-only.
    pub fn cancel_upgrade(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin mutations
    // -----------------------------------------------------------------------

    /// Append a new `check` to the end of the policy list. Admin-only.
    ///
    /// Returns `Err(Error::MaxDepthExceeded)` if the list already contains
    /// `MAX_CHECKS` entries. This keeps the number of cross-contract calls
    /// issued by `evaluate` bounded and prevents storage bloat.
    pub fn add_check(env: Env, admin: Address, check: CheckKind) -> Result<(), Error> {
        compliance_pausable::require_not_paused_or(&env, Error::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        let mut checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        if checks.len() >= MAX_CHECKS {
            return Err(Error::MaxDepthExceeded);
        }
        checks.push_back(check);
        env.storage().instance().set(&DataKey::Checks, &checks);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Remove the check at position `index` from the policy list.
    /// Admin-only. Indices shift down after removal (Vec::remove semantics).
    pub fn remove_check(env: Env, admin: Address, index: u32) -> Result<(), Error> {
        compliance_pausable::require_not_paused_or(&env, Error::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        let mut checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        if index >= checks.len() {
            return Err(Error::IndexOutOfBounds);
        }
        checks.remove(index);
        env.storage().instance().set(&DataKey::Checks, &checks);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Swap the checks at positions `i` and `j` in the policy list.
    /// Admin-only.
    ///
    /// Issuers can use this to reorder checks without having to remove and
    /// re-add them (which would also change their indices). A common use case
    /// is placing the cheapest check first so it short-circuits early under
    /// `CombineOp::All`, avoiding unnecessary cross-contract calls.
    ///
    /// Returns `Err(Error::IndexOutOfBounds)` if either index is out of range.
    /// No-ops if `i == j`.
    pub fn swap_checks(env: Env, admin: Address, i: u32, j: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if i == j {
            return Ok(());
        }
        let mut checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        let len = checks.len();
        if i >= len || j >= len {
            return Err(Error::IndexOutOfBounds);
        }
        let check_i = checks.get(i).unwrap();
        let check_j = checks.get(j).unwrap();
        checks.set(i, check_j);
        checks.set(j, check_i);
        env.storage().instance().set(&DataKey::Checks, &checks);
        Ok(())
    }

    /// Remove every configured check in one call. Admin-only.
    pub fn clear_checks(env: Env, admin: Address) -> Result<(), Error> {
        compliance_pausable::require_not_paused_or(&env, Error::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        let empty: Vec<CheckKind> = Vec::new(&env);
        env.storage().instance().set(&DataKey::Checks, &empty);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Evaluation
    // -----------------------------------------------------------------------

    /// Evaluate the current policy for a proposed transfer from `from` to
    /// `to`. Runs every registered check against both addresses using the
    /// configured `CombineOp`.
    ///
    /// Returns `Ok(true)` if the policy passes, `Ok(false)` if it fails.
    /// A `PolicyResult` event is always emitted so the outcome is auditable
    /// on-chain regardless of the result. `Err` is returned only for
    /// configuration failures (e.g. the contract was never initialized).
    pub fn evaluate(env: Env, from: Address, to: Address) -> Result<bool, Error> {
        let checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        let op: CombineOp = env
            .storage()
            .instance()
            .get(&DataKey::CombineOp)
            .ok_or(Error::NotInitialized)?;
        let (passed, _) = Self::evaluate_inner(&env, &checks, &op, &from, &to);
        Ok(passed)
    }

    /// Evaluate the current policy for multiple address pairs (transfers) in a single call.
    ///
    /// Returns a `Vec<bool>` containing the pass/fail status for each pair in the same order.
    pub fn batch_evaluate(env: Env, pairs: Vec<AddressPair>) -> Result<Vec<bool>, Error> {
        let checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        let op: CombineOp = env
            .storage()
            .instance()
            .get(&DataKey::CombineOp)
            .ok_or(Error::NotInitialized)?;
        let mut results = Vec::new(&env);
        for pair in pairs.iter() {
            let (passed, _) = Self::evaluate_inner(&env, &checks, &op, &pair.from, &pair.to);
            results.push_back(passed);
        }
        Ok(results)
    }

    /// Like `evaluate`, but on failure also returns a [`CheckFailure`]
    /// describing the **first** check that caused the policy to fail.
    ///
    /// Returns `Ok((true, None))` when the policy passes. Returns
    /// `Ok((false, Some(CheckFailure { check_index, kind })))` when the
    /// policy fails, giving callers the zero-based index and the `Symbol`
    /// name of the failing check kind without needing a second round-trip.
    ///
    /// A `PolicyResult` event is emitted just as in `evaluate`, keeping the
    /// audit trail intact regardless of the outcome. The circuit-breaker
    /// short-circuit (if configured) is still respected and produces
    /// `Ok((false, None))` — no index is surfaced in that case because the
    /// freeze is a system-wide condition rather than a per-check failure.
    pub fn evaluate_verbose(
        env: Env,
        from: Address,
        to: Address,
    ) -> Result<(bool, Option<CheckFailure>), Error> {
        let checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        let op: CombineOp = env
            .storage()
            .instance()
            .get(&DataKey::CombineOp)
            .ok_or(Error::NotInitialized)?;
        let result = Self::evaluate_inner(&env, &checks, &op, &from, &to);
        Ok(result)
    }

    fn evaluate_inner(
        env: &Env,
        checks: &Vec<CheckKind>,
        op: &CombineOp,
        from: &Address,
        to: &Address,
    ) -> (bool, Option<CheckFailure>) {
        let breaker_addr: Option<Address> = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::CircuitBreaker);
        if let Some(addr) = breaker_addr {
            if CircuitBreakerClient::new(env, &addr).is_frozen() {
                PolicyResult {
                    passed: false,
                    from: from.clone(),
                    to: to.clone(),
                }
                .publish(env);
                return (false, None);
            }
        }

        let result = match op {
            CombineOp::All => {
                let mut result = (true, None);
                for i in 0..checks.len() {
                    let check = checks.get(i).unwrap();
                    if !Self::run_check(env, &check, from) || !Self::run_check(env, &check, to) {
                        result = (
                            false,
                            Some(CheckFailure {
                                check_index: i,
                                kind: Self::check_kind_symbol(env, &check),
                            }),
                        );
                        break;
                    }
                }
                result
            }
            CombineOp::Any => {
                if checks.is_empty() {
                    (false, None)
                } else {
                    let mut any_pass = false;
                    for i in 0..checks.len() {
                        let check = checks.get(i).unwrap();
                        if Self::run_check(env, &check, from) && Self::run_check(env, &check, to) {
                            any_pass = true;
                            break;
                        }
                    }
                    if any_pass {
                        (true, None)
                    } else {
                        let first = checks.get(0).unwrap();
                        (
                            false,
                            Some(CheckFailure {
                                check_index: 0,
                                kind: Self::check_kind_symbol(env, &first),
                            }),
                        )
                    }
                }
            }
        };

        PolicyResult {
            passed: result.0,
            from: from.clone(),
            to: to.clone(),
        }
        .publish(env);
        result
    }

    // -----------------------------------------------------------------------
    // Read-only accessors
    // -----------------------------------------------------------------------

    /// Returns the check at position `index` in the registered policy list,
    /// or `Err(Error::CheckIndexOutOfRange)` if `index` is out of range.
    ///
    /// This is more efficient than `get_checks()` when a caller only needs a
    /// single entry (e.g. to inspect or display the config of one rule in a
    /// UI), because it avoids deserializing and returning the entire
    /// `Vec<CheckKind>` — especially important as the list grows toward
    /// `MAX_CHECKS`.
    pub fn get_check(env: Env, index: u32) -> Result<CheckKind, Error> {
        let checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        checks.get(index).ok_or(Error::CheckIndexOutOfRange)
    }

    /// Returns the current admin address, or `Err(Error::NotInitialized)` if
    /// the contract has not yet been initialized.
    ///
    /// Useful for off-chain tooling, UIs, and other contracts that need to
    /// verify who controls this policy-engine instance without having to
    /// replay the initialization transaction.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Returns the current list of registered checks.
    pub fn get_checks(env: Env) -> Vec<CheckKind> {
        env.storage()
            .instance()
            .get(&DataKey::Checks)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the current combination operator.
    pub fn get_op(env: Env) -> Result<CombineOp, Error> {
        env.storage()
            .instance()
            .get(&DataKey::CombineOp)
            .ok_or(Error::NotInitialized)
    }

    /// Returns the full configured policy tree as a [`PolicyNode`].
    ///
    /// Off-chain tooling and auditors can call this single view to read back
    /// everything needed to understand and reproduce the policy that a
    /// deployed instance will evaluate, without having to reconstruct it from
    /// deployment transaction history.
    ///
    /// Returns `Err(Error::NotInitialized)` if the contract has not yet been
    /// initialized.
    pub fn get_policy(env: Env) -> Result<PolicyNode, Error> {
        let op: CombineOp = env
            .storage()
            .instance()
            .get(&DataKey::CombineOp)
            .ok_or(Error::NotInitialized)?;
        let checks: Vec<CheckKind> = env
            .storage()
            .instance()
            .get(&DataKey::Checks)
            .ok_or(Error::NotInitialized)?;
        Ok(PolicyNode { op, checks })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn run_check(env: &Env, check: &CheckKind, address: &Address) -> bool {
        match check {
            CheckKind::Denylist(params) => {
                let client = DenylistCheckClient::new(env, &params.contract);
                client.check(address)
            }
            CheckKind::Jurisdiction(params) => {
                let client = JurisdictionCheckClient::new(env, &params.contract);
                // A callee error (e.g. `EmptyAllowedCodes`) fails the check.
                matches!(
                    client.try_is_permitted_jurisdiction(address, &params.allowed_codes),
                    Ok(Ok(true))
                )
            }
            CheckKind::Allowlist(params) => {
                let client = AllowlistCheckClient::new(env, &params.contract);
                client.is_allowed(address)
            }
            CheckKind::CircuitBreaker(params) => {
                // The circuit-breaker check is address-independent: it reflects
                // the global freeze state of the system. The check passes only
                // when the breaker is NOT frozen.
                let _ = address; // address not used for this check kind
                let client = CircuitBreakerClient::new(env, &params.contract);
                !client.is_frozen()
            }
        }
    }

    /// Returns a short `Symbol` naming the `CheckKind` variant for use in
    /// `CheckFailure`. Kept in sync with the enum variants.
    fn check_kind_symbol(env: &Env, check: &CheckKind) -> Symbol {
        match check {
            CheckKind::Denylist(_) => Symbol::new(env, "Denylist"),
            CheckKind::Jurisdiction(_) => Symbol::new(env, "Jurisdiction"),
            CheckKind::Allowlist(_) => Symbol::new(env, "Allowlist"),
            CheckKind::CircuitBreaker(_) => Symbol::new(env, "CircuitBreaker"),
        }
    }

    /// Refresh the TTL of the contract instance (and therefore of every
    /// instance-storage entry). See the "Storage and TTL policy" section of
    /// the module docs.
    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
    }

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
mod test_utils;

#[cfg(test)]
mod test;

#[cfg(test)]
mod fuzz;

#[cfg(test)]
mod bench;

