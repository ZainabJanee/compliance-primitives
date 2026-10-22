// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! Reference example: a minimal token contract that composes
//! `circuit-breaker` via cross-contract call. This crate is not meant to be
//! deployed as-is — it exists to show the calling pattern other issuers'
//! token contracts should follow: call `is_frozen()` on the breaker before
//! mutating any balances, and abort the transfer if the breaker is tripped.
//!
//! Note this deliberately does NOT depend on the `circuit-breaker` crate
//! directly: linking another contract's crate pulls its `#[contractimpl]`
//! wasm exports into this binary too and the two export sets collide at
//! link time. Instead, `BreakerClient` below is generated from a trait that
//! only describes the shape of the call — the standard pattern for calling
//! a contract you don't own the source of in the same build.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env,
};

#[contractclient(name = "BreakerClient")]
pub trait CircuitBreakerInterface {
    fn is_frozen(env: Env) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
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
    FrozenByBreaker = 4,
}

#[contract]
pub struct ExampleToken;

#[contractimpl]
impl ExampleToken {
    /// `breaker` is the address of a deployed `circuit-breaker` contract.
    pub fn initialize(env: Env, breaker: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Breaker) {
            return Err(Error::AlreadyInitialized);
        }
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

    /// Transfer `amount` from `from` to `to`, gated by `circuit-breaker`.
    ///
    /// This is the pattern to copy when wiring an emergency stop into your
    /// own token contract:
    ///
    /// 1. Resolve the breaker's address from this contract's own storage
    ///    (set once at `initialize`) rather than hardcoding it, so the same
    ///    contract code can point at different breaker deployments.
    /// 2. Build a `BreakerClient` for that address. `BreakerClient` is
    ///    generated from the `CircuitBreakerInterface` trait above, not from
    ///    the `circuit-breaker` crate itself (see the module docs for why).
    /// 3. Call `is_frozen()` before touching any balances. While the breaker
    ///    is tripped, every transfer is rejected regardless of sender,
    ///    recipient, or amount.
    /// 4. Abort before mutating any balances if the breaker is frozen.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        // Step 1: resolve the breaker's address from storage.
        let breaker_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Breaker)
            .ok_or(Error::NotInitialized)?;
        // Step 2: build a client for the deployed breaker contract.
        let breaker = BreakerClient::new(&env, &breaker_address);

        // Step 3: check the emergency stop before anything else.
        if breaker.is_frozen() {
            return Err(Error::FrozenByBreaker);
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
