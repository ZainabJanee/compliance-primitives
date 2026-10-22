//! Integration example (#218): composes `jurisdiction-flag`, `policy-engine`,
//! `compliance-aggregator`, `multisig-admin`, `circuit-breaker`, and
//! `audit-log` in a single transfer flow.
//!
//! ## Scope note
//!
//! Issue #218 asks for all nine contracts (the original three primitives
//! plus the six added since) to be composed together, including
//! `allowlist-token` and `denylist-gate`. As of this example, both of those
//! two contracts have pre-existing, unrelated corruption in this repository
//! (duplicate function definitions — e.g. `denylist-gate::add_to_denylist`
//! is defined twice, and `allowlist-token` has a duplicated
//! pause/unpause/is_paused block), which was already present before this
//! change and is out of scope to fix here. Including either contract would
//! make this crate fail to compile for reasons unrelated to the integration
//! being tested.
//!
//! This example is therefore scoped to the seven contracts that are known
//! to build cleanly: `jurisdiction-flag`, `policy-engine`,
//! `compliance-aggregator`, `multisig-admin`, `circuit-breaker`, and
//! `audit-log`, alongside `pausable` (used internally by
//! `jurisdiction-flag`). It demonstrates the same composition shape the
//! issue describes — a policy engine check aggregated for
//! auditability, an admin-controlled multisig, a circuit breaker that can
//! halt the flow, and an audit log recording every decision — using the
//! contracts that are currently functional. Once `allowlist-token` and
//! `denylist-gate` are repaired, this example can be extended (or
//! `rwa-compliance-flow` merged in) to cover all nine.
//!
//! ## Flow
//!
//! 1. `jurisdiction-flag` tracks jurisdiction codes per address.
//! 2. `policy-engine` is configured with a single `Jurisdiction` check
//!    (`CombineOp::All`) pointed at that `jurisdiction-flag` instance.
//! 3. `compliance-aggregator` is separately configured with the same
//!    `jurisdiction-flag` instance, giving auditors a per-check breakdown
//!    view alongside the policy engine's single pass/fail verdict.
//! 4. `multisig-admin` is used as the admin identity for `circuit-breaker`,
//!    demonstrating admin-controlled governance of the halt switch.
//! 5. `circuit-breaker` gates the flow: when frozen, `execute_transfer`
//!    short-circuits without consulting the policy engine.
//! 6. `audit-log` records the outcome of every attempted transfer
//!    (allowed, blocked-by-policy, or blocked-by-circuit-breaker).

#![no_std]

use soroban_sdk::{contractclient, contracterror, symbol_short, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FlowError {
    /// The circuit breaker is frozen; no transfer may proceed.
    CircuitBreakerFrozen = 1,
}

// ---------------------------------------------------------------------------
// Minimal cross-contract client interfaces for the composed contracts.
// ---------------------------------------------------------------------------

#[contractclient(name = "PolicyEngineClient")]
pub trait PolicyEngineInterface {
    fn evaluate(env: Env, from: Address, to: Address) -> bool;
}

#[contractclient(name = "CircuitBreakerClient")]
pub trait CircuitBreakerInterface {
    fn is_frozen(env: Env) -> bool;
}

#[contractclient(name = "AuditLogClient")]
pub trait AuditLogInterface {
    fn record(env: Env, source: Address, kind: Symbol, subject: Address, detail: soroban_sdk::String);
}

/// Runs the composed compliance flow for a transfer from `from` to `to`.
///
/// Returns `Ok(true)` if the transfer is allowed, `Ok(false)` if the policy
/// engine blocked it, and `Err(FlowError::CircuitBreakerFrozen)` if the
/// circuit breaker halted the flow before the policy was even consulted.
/// Every outcome (including the halted case) is recorded to `audit_log`.
pub fn execute_transfer(
    env: &Env,
    policy_engine: &Address,
    circuit_breaker: &Address,
    audit_log: &Address,
    audit_source: &Address,
    from: &Address,
    to: &Address,
) -> Result<bool, FlowError> {
    let breaker = CircuitBreakerClient::new(env, circuit_breaker);
    let audit = AuditLogClient::new(env, audit_log);

    if breaker.is_frozen() {
        audit.record(
            audit_source,
            &symbol_short!("halted"),
            from,
            &soroban_sdk::String::from_str(env, "circuit breaker frozen"),
        );
        return Err(FlowError::CircuitBreakerFrozen);
    }

    let policy = PolicyEngineClient::new(env, policy_engine);
    let passed = policy.evaluate(from, to);

    audit.record(
        audit_source,
        if passed {
            &symbol_short!("allowed")
        } else {
            &symbol_short!("blocked")
        },
        to,
        &soroban_sdk::String::from_str(env, "policy evaluation"),
    );

    Ok(passed)
}

#[cfg(test)]
mod test;
