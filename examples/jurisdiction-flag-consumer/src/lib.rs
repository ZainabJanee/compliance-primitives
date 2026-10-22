//! Reference example: a minimal token contract that composes
//! `jurisdiction-flag` via cross-contract call. This crate is not meant to be
//! deployed as-is — it exists to show the calling pattern other issuers'
//! token contracts should follow: call `is_permitted_jurisdiction()` on the
//! flag for addresses subject to jurisdiction restrictions before mutating
//! any balances, and abort the transfer if the check fails.
//!
//! **Composition choice**: `transfer` checks only the **sender's** jurisdiction
//! against the configured `allowed_codes`. The sender is the party whose KYC /
//! jurisdiction verification matters for outbound transfers; the recipient is
//! not re-checked here (a receiving contract could apply its own policy).
//!
//! **Trait-based cross-contract calls**:
//! Note this deliberately does NOT depend on the `jurisdiction-flag` crate
//! directly: linking another contract's crate pulls its `#[contractimpl]`
//! wasm exports into this binary too and the two export sets collide at
//! link time. Instead, `FlagClient` below is generated from a trait that
//! only describes the shape of the call — the standard pattern for calling
//! a contract you don't own the source of in the same build.
//!
//! ## How to compose `jurisdiction-flag` into your token
//!
//! 1. **Resolve the flag's address** from this contract's own storage (set
//!    once at `initialize`) rather than hardcoding it, so the same contract
//!    code can point at different flag deployments.
//! 2. **Build a `FlagClient`** for that address. `FlagClient` is generated
//!    from the `JurisdictionFlagInterface` trait above, not from the
//!    `jurisdiction-flag` crate itself (see above for why), so this step
//!    works without a direct crate dependency.
//! 3. **Call `is_permitted_jurisdiction()`** for the address(es) you want to
//!    check, passing the list of allowed jurisdiction codes. Each call is a
//!    cross-contract invocation into the deployed flag contract — the check
//!    logic itself lives entirely in `jurisdiction-flag`, this contract just
//!    asks it for a yes/no per address.
//! 4. **Abort before mutating any balances** if the check fails. The check
//!    must run and pass; failing the jurisdiction check is sufficient to
//!    reject the transfer, and balances must stay untouched.
#![no_std]

use soroban_sdk::{contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, String, Vec};

#[contractclient(name = "FlagClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Flag,
    AllowedCodes,
    Balance(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    JurisdictionNotPermitted = 4,
}

#[contract]
pub struct ExampleToken;

#[contractimpl]
impl ExampleToken {
    /// `flag` is a deployed `jurisdiction-flag` instance; `allowed_codes` is the
    /// set of jurisdiction codes this token permits for senders.
    pub fn initialize(
        env: Env,
        flag: Address,
        allowed_codes: Vec<String>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Flag) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Flag, &flag);
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

    /// Transfer `amount` from `from` to `to`, gated by `jurisdiction-flag`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        let flag_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Flag)
            .ok_or(Error::NotInitialized)?;
        let allowed_codes: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedCodes)
            .ok_or(Error::NotInitialized)?;
        let flag = FlagClient::new(&env, &flag_address);

        let permitted = matches!(
            flag.try_is_permitted_jurisdiction(&from, &allowed_codes),
            Ok(Ok(true))
        );
        if !permitted {
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
