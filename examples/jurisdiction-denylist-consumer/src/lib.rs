//! Reference example: a token contract that composes both `denylist-gate`
//! and `jurisdiction-flag` via cross-contract calls.
//!
//! **Check Order & Behavior**:
//! 1. `denylist-gate`: Calls `check()` for both sender (`from`) and recipient (`to`).
//!    If either party is denylisted, transfer aborts immediately with `Error::DeniedByGate`.
//! 2. `jurisdiction-flag`: Calls `is_permitted_jurisdiction()` for the sender (`from`)
//!    against the configured list of allowed jurisdiction codes.
//!    If the sender's jurisdiction is missing or not permitted, transfer aborts with `Error::DeniedByJurisdiction`.
//! 3. Balance check: Verifies sender has sufficient balance (`Error::InsufficientBalance`).
//! 4. Balance update: Decrements sender balance and increments recipient balance.
//!
//! **Trait-based cross-contract calls**:
//! This crate does not directly depend on `denylist-gate` or `jurisdiction-flag` in its core dependencies.
//! Instead, `#[contractclient]`-generated client traits (`GateClient` and `JurisdictionFlagClient`)
//! are used to invoke the external contracts without crate link-time collision.

#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, String, Vec,
};

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "JurisdictionFlagClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Gate,
    Jurisdiction,
    AllowedJurisdictions,
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
    DeniedByJurisdiction = 5,
}

#[contract]
pub struct JurisdictionDenylistConsumer;

#[contractimpl]
impl JurisdictionDenylistConsumer {
    /// Initialize the contract with the deployed `denylist-gate` and `jurisdiction-flag` addresses
    /// as well as the initial list of permitted jurisdiction codes.
    pub fn initialize(
        env: Env,
        gate: Address,
        jurisdiction: Address,
        allowed_jurisdictions: Vec<String>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Gate) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage()
            .instance()
            .set(&DataKey::Jurisdiction, &jurisdiction);
        env.storage()
            .instance()
            .set(&DataKey::AllowedJurisdictions, &allowed_jurisdictions);
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

    /// Transfer `amount` from `from` to `to`, gated by both `denylist-gate` (both parties)
    /// and `jurisdiction-flag` (sender).
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        let gate_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        let jurisdiction_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Jurisdiction)
            .ok_or(Error::NotInitialized)?;
        let allowed_jurisdictions: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedJurisdictions)
            .ok_or(Error::NotInitialized)?;

        let gate = GateClient::new(&env, &gate_address);
        if !gate.check(&from) || !gate.check(&to) {
            return Err(Error::DeniedByGate);
        }

        let jurisdiction = JurisdictionFlagClient::new(&env, &jurisdiction_address);
        let permitted = matches!(
            jurisdiction.try_is_permitted_jurisdiction(&from, &allowed_jurisdictions),
            Ok(Ok(true))
        );
        if !permitted {
            return Err(Error::DeniedByJurisdiction);
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
