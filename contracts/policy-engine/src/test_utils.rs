//! Shared mock contracts and helpers used by both `test` and `fuzz` modules.

use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec};

// ---------------------------------------------------------------------------
// Inline mock: denylist check
//
// `check(address)` returns `true` iff the address is NOT denied, matching
// the `DenylistCheckInterface` used by `policy-engine`.
// ---------------------------------------------------------------------------

#[contract]
pub struct MockDenylist;

#[contractimpl]
impl MockDenylist {
    /// Mark `address` as denied.
    pub fn add_to_denylist(env: Env, address: Address) {
        env.storage().persistent().set(&address, &true);
    }

    /// Returns `true` if address is NOT denied.
    pub fn check(env: Env, address: Address) -> bool {
        !env.storage()
            .persistent()
            .get::<Address, bool>(&address)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Inline mock: jurisdiction check
//
// Stores a single jurisdiction code per address.
// `is_permitted_jurisdiction` returns `true` iff the stored code is in the
// allowed list, matching `JurisdictionCheckInterface`.
// ---------------------------------------------------------------------------

#[contract]
pub struct MockJurisdiction;

#[contractimpl]
impl MockJurisdiction {
    /// Assign `code` to `address`.
    pub fn set_jurisdiction(env: Env, address: Address, code: String) {
        env.storage().persistent().set(&address, &code);
    }

    /// Returns `true` iff the stored code for `address` is in `allowed_codes`.
    pub fn is_permitted_jurisdiction(
        env: Env,
        address: Address,
        allowed_codes: Vec<String>,
    ) -> bool {
        match env
            .storage()
            .persistent()
            .get::<Address, String>(&address)
        {
            Some(code) => allowed_codes.iter().any(|c| c == code),
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Inline mock: circuit-breaker check
//
// `is_frozen()` returns the stored frozen state, matching
// `CircuitBreakerInterface` used by `policy-engine`.
// ---------------------------------------------------------------------------

#[contract]
pub struct MockCircuitBreaker;

#[contractimpl]
impl MockCircuitBreaker {
    /// Freeze the circuit-breaker (causes `is_frozen` to return `true`).
    pub fn freeze(env: Env) {
        env.storage().instance().set(&true, &true);
    }

    /// Unfreeze the circuit-breaker.
    pub fn unfreeze(env: Env) {
        env.storage().instance().set(&true, &false);
    }

    /// Returns `true` if the circuit-breaker has been frozen.
    pub fn is_frozen(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<bool, bool>(&true)
            .unwrap_or(false)
    }
}
