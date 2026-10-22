//! # RWA Token Example
//!
//! A complete reference implementation of a Real-World Asset (RWA) token that
//! composes all three compliance primitives (allowlist, denylist, jurisdiction)
//! before mutating balances.
//!
//! ## Composition Pattern
//!
//! This example demonstrates a **serial composition** of three compliance gates
//! in a fail-fast pattern. Unlike `/examples/rwa-compliance-flow` (which is a
//! test-oriented integration showing composition semantics) or
//! `/examples/denylist-gate-sep41` (which is a SEP-41 token gated by a single
//! denylist), this crate shows a **full, production-like token implementation**
//! that:
//!
//! 1. Implements custom balance tracking (not relying on SEP-41 out-of-the-box)
//! 2. Wires three compliance primitives as dependencies
//! 3. Applies all three checks in series during `transfer` with specific error
//!    handling for each gate
//!
//! Check order in `transfer` (fail-fast, specific error per gate):
//! 1. **Allowlist** — both `from` and `to` must pass `is_allowed`
//! 2. **Denylist** — both parties must pass `denylist-gate.check`
//! 3. **Jurisdiction** — both parties must pass
//!    `is_permitted_jurisdiction` against the token's configured
//!    `allowed_codes`
//!
//! After all compliance checks clear, the balance mutation succeeds.
//!
//! ## Implementation Details
//!
//! Like `/examples/denylist-gate-consumer`, this crate does **not** depend
//! on the primitive crates' `#[contractimpl]` binaries at link time.
//! Clients are generated from `#[contractclient]` traits that only describe
//! the call shape. This avoids linker symbol collisions and keeps each contract's
//! wasm binary self-contained.
//!
//! The `mint` helper method is a non-standard addition (not part of the token
//! spec) used for testing and demonstration; a real RWA token would use an
//! external minting process.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, String, Vec,
};

#[contractclient(name = "AllowlistClient")]
pub trait AllowlistInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "JurisdictionClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

#[contractclient(name = "CircuitBreakerClient")]
pub trait CircuitBreakerInterface {
    fn is_frozen(env: Env) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Allowlist,
    Gate,
    Jurisdiction,
    AllowedCodes,
    Balance(Address),
    CircuitBreaker,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    NotAllowlisted = 4,
    DeniedByGate = 5,
    JurisdictionNotPermitted = 6,
    CircuitBreakerFrozen = 7,
}

#[contract]
pub struct RwaToken;

#[contractimpl]
impl RwaToken {
    /// Wire this token to deployed primitive instances and the set of
    /// jurisdiction codes that may hold/transfer it.
    pub fn initialize(
        env: Env,
        allowlist: Address,
        gate: Address,
        jurisdiction: Address,
        circuit_breaker: Address,
        allowed_codes: Vec<String>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Allowlist) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Allowlist, &allowlist);
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage()
            .instance()
            .set(&DataKey::Jurisdiction, &jurisdiction);
        env.storage()
            .instance()
            .set(&DataKey::CircuitBreaker, &circuit_breaker);
        env.storage()
            .instance()
            .set(&DataKey::AllowedCodes, &allowed_codes);
        Ok(())
    }

    /// Test/demo helper to fund an address with an initial balance.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let balance = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(balance + amount));
    }

    pub fn balance(env: Env, address: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(address))
            .unwrap_or(0)
    }

    /// Transfer `amount` from `from` to `to` after all compliance
    /// checks clear. Errors identify which gate failed.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        let circuit_breaker_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CircuitBreaker)
            .ok_or(Error::NotInitialized)?;
        let circuit_breaker = CircuitBreakerClient::new(&env, &circuit_breaker_addr);
        if circuit_breaker.is_frozen() {
            return Err(Error::CircuitBreakerFrozen);
        }

        let allowlist_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .ok_or(Error::NotInitialized)?;
        let gate_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        let jurisdiction_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Jurisdiction)
            .ok_or(Error::NotInitialized)?;
        let allowed_codes: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedCodes)
            .ok_or(Error::NotInitialized)?;

        let allowlist = AllowlistClient::new(&env, &allowlist_addr);
        if !allowlist.is_allowed(&from) || !allowlist.is_allowed(&to) {
            return Err(Error::NotAllowlisted);
        }

        let gate = GateClient::new(&env, &gate_addr);
        if !gate.check(&from) || !gate.check(&to) {
            return Err(Error::DeniedByGate);
        }

        let jurisdiction = JurisdictionClient::new(&env, &jurisdiction_addr);
        let from_permitted = matches!(
            jurisdiction.try_is_permitted_jurisdiction(&from, &allowed_codes),
            Ok(Ok(true))
        );
        let to_permitted = matches!(
            jurisdiction.try_is_permitted_jurisdiction(&to, &allowed_codes),
            Ok(Ok(true))
        );
        if !from_permitted || !to_permitted {
            return Err(Error::JurisdictionNotPermitted);
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(Error::InsufficientBalance);
        }
        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));
        Ok(())
    }
}

#[cfg(test)]
mod test;
