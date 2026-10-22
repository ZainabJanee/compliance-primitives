// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! Reference example: a minimal token contract that composes
//! `denylist-gate` via cross-contract call. This crate is not meant to be
//! deployed as-is — it exists to show the calling pattern other issuers'
//! token contracts should follow: call `check()` on the gate for both
//! parties before mutating any balances, and abort the transfer if either
//! check fails.
//!
//! Note this deliberately does NOT depend on the `denylist-gate` crate
//! directly: linking another contract's crate pulls its `#[contractimpl]`
//! wasm exports into this binary too and the two export sets collide at
//! link time. Instead, `GateClient` below is generated from a trait that
//! only describes the shape of the call — the standard pattern for calling
//! a contract you don't own the source of in the same build.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env,
};

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "BreakerClient")]
pub trait CircuitBreakerInterface {
    fn is_frozen(env: Env) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Gate,
    Breaker,
    Balance(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    DeniedByGate = 4,
    FrozenByBreaker = 5,
}

#[contract]
pub struct ExampleToken;

#[contractimpl]
impl ExampleToken {
    /// `gate` is the address of a deployed `denylist-gate` contract instance.
    /// `breaker` is the address of a deployed `circuit-breaker` contract.
    pub fn initialize(env: Env, gate: Address, breaker: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Gate) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage().instance().set(&DataKey::Breaker, &breaker);
        Ok(())
    }

    /// Test/demo helper to fund an address with an initial balance.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let balance = Self::balance(env.clone(), to.clone());
        env.storage().persistent().set(&DataKey::Balance(to), &(balance + amount));
    }

    pub fn balance(env: Env, address: Address) -> i128 {
        env.storage().persistent().get(&DataKey::Balance(address)).unwrap_or(0)
    }

    /// Transfer `amount` from `from` to `to`, gated by `denylist-gate`.
    ///
    /// This is the pattern to copy when composing a `denylist-gate`
    /// instance into your own token contract:
    ///
    /// 1. Resolve the gate's address from this contract's own storage
    ///    (set once at `initialize`) rather than hardcoding it, so the same
    ///    contract code can point at different gate deployments.
    /// 2. Build a `GateClient` for that address. `GateClient` is generated
    ///    from the `DenylistGateInterface` trait above, not from the
    ///    `denylist-gate` crate itself (see the module docs for why), so
    ///    this step works without a direct crate dependency.
    /// 3. Call `check()` for both `from` and `to`. Each call is a
    ///    cross-contract invocation into the deployed gate contract — the
    ///    check logic itself lives entirely in `denylist-gate`, this
    ///    contract just asks it for a yes/no per address.
    /// 4. Abort before mutating any balances if either check fails. Both
    ///    checks must run and both must pass; failing on the sender OR the
    ///    recipient is sufficient to reject the transfer, and balances must
    ///    stay untouched either way.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        // Step 1: resolve the gate's address from storage.
        let gate_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        // Step 2: build a client for the deployed gate contract.
        let gate = GateClient::new(&env, &gate_address);

        let breaker_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Breaker)
            .ok_or(Error::NotInitialized)?;
        let breaker = BreakerClient::new(&env, &breaker_address);

        if breaker.is_frozen() {
            return Err(Error::FrozenByBreaker);
        }

        if !gate.check(&from) || !gate.check(&to) {
            return Err(Error::DeniedByGate);
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(Error::InsufficientBalance);
        }
        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_balance + amount));
        Ok(())
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod fuzz_test;
