// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! `jurisdiction-flag` is a `#![no_std]` Soroban contract that attaches a
//! jurisdiction code (e.g. an ISO 3166-1 alpha-2 country code) to an
//! address.
//!
//! **Permission semantics**: `is_permitted_jurisdiction` uses *any*
//! matching — it returns `true` if at least one of the address's codes
//! appears in `allowed_codes`. An address with no codes is never permitted.
//!
//! **Callers**: only the configured `issuer` address may call
//! `set_jurisdiction` / `remove_jurisdiction_multiple`. Any contract or
//! off-chain client can read a flag via `get_jurisdiction`, and contracts
//! enforcing a jurisdiction allowlist can call
//! `is_permitted_jurisdiction(address, allowed_codes)` directly.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    String, Vec,
};

/// Extend persistent jurisdiction entries when TTL drops below this many ledgers.
const TTL_THRESHOLD: u32 = 1_000;
/// Target TTL (in ledgers) after extension.
const TTL_EXTEND_TO: u32 = 5_000;

/// Maximum number of entries accepted by `set_multiple_jurisdictions` in a
/// single call.  Keeps transaction costs bounded and prevents resource
/// exhaustion.
pub const MAX_BATCH_SIZE: u32 = 100;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The issuer address, set once in `initialize`. Instance storage.
    Issuer,
    ComplianceOfficer,
    Jurisdiction(Address),
    Paused,
    /// Pending issuer address proposed via `propose_issuer`. Instance storage.
    PendingIssuer,
}

/// Emitted whenever a jurisdiction flag is set.
#[contractevent]
pub struct JurisdictionSet {
    #[topic]
    pub address: Address,
    pub code: String,
}

#[contractevent]
pub struct JurisdictionRemoved {
    #[topic]
    pub address: Address,
}

#[contractevent]
pub struct Paused {
    #[topic]
    pub issuer: Address,
}

#[contractevent]
pub struct Unpaused {
    #[topic]
    pub issuer: Address,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    /// Caller supplied an argument that is structurally invalid.
    InvalidInput = 4,
    ContractPaused = 5,
    /// `is_permitted_jurisdiction` was called with an empty `allowed_codes`
    /// list, which would always return false and is almost certainly a
    /// caller mistake.
    EmptyAllowedCodes = 6,
    /// The pending-issuer key is absent; `accept_issuer` was called when
    /// no transfer is in progress.
    NoPendingIssuer = 7,
    /// The address calling `accept_issuer` does not match the pending issuer
    /// that was stored by `propose_issuer`.
    PendingIssuerMismatch = 8,
}

#[contract]
pub struct JurisdictionFlag;

#[contractimpl]
impl JurisdictionFlag {
    /// One-time setup. `issuer` is the only address allowed to set
    /// jurisdiction codes afterward.
    pub fn initialize(env: Env, issuer: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Issuer) {
            return Err(Error::AlreadyInitialized);
        }
        issuer.require_auth();
        env.storage().instance().set(&DataKey::Issuer, &issuer);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Assign the compliance-officer role. Issuer-only.
    pub fn set_compliance_officer(
        env: Env,
        issuer: Address,
        officer: Address,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage()
            .instance()
            .set(&DataKey::ComplianceOfficer, &officer);
        Ok(())
    }

    /// Revoke the compliance-officer role. Issuer-only.
    pub fn revoke_compliance_officer(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage()
            .instance()
            .remove(&DataKey::ComplianceOfficer);
        Ok(())
    }

    /// Pause all mutating operations. Issuer-only.
    pub fn pause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Paused {
            issuer: issuer.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Resume all mutating operations. Issuer-only.
    pub fn unpause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Unpaused {
            issuer: issuer.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Attach jurisdiction `code` to `address`. Issuer or compliance-officer.
    pub fn set_jurisdiction(
        env: Env,
        caller: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_compliance_authority(&env, &caller)?;
        Self::require_not_paused(&env)?;
        Self::validate_code(&code)?;

        let key = DataKey::Jurisdiction(address.clone());
        env.storage().persistent().set(&key, &code);
        Self::extend_jurisdiction_ttl(&env, &key);

        JurisdictionSet { address, code }.publish(&env);
        Ok(())
    }

    /// Batch-set jurisdiction codes for multiple addresses in one transaction.
    ///
    /// `caller` must be the issuer or the compliance officer.  All entries are
    /// validated **before** any writes, so one invalid code rejects the whole
    /// batch without leaving partial state.  The batch is capped at
    /// [`MAX_BATCH_SIZE`] entries.
    ///
    /// Added to resolve issue #604 — the missing batch setter that prevented
    /// efficient cohort onboarding.
    pub fn set_multiple_jurisdictions(
        env: Env,
        caller: Address,
        entries: Vec<(Address, String)>,
    ) -> Result<(), Error> {
        Self::require_compliance_authority(&env, &caller)?;
        Self::require_not_paused(&env)?;

        // Reject empty or over-cap batches.
        if entries.is_empty() || entries.len() as u32 > MAX_BATCH_SIZE {
            return Err(Error::InvalidInput);
        }

        // Validate ALL codes before writing anything so that a single bad
        // entry fails the entire batch cleanly (no partial state).
        for (_, code) in entries.iter() {
            Self::validate_code(&code)?;
        }

        // All entries passed validation — now write.
        for (address, code) in entries.iter() {
            let key = DataKey::Jurisdiction(address.clone());
            env.storage().persistent().set(&key, &code);
            Self::extend_jurisdiction_ttl(&env, &key);
            JurisdictionSet {
                address: address.clone(),
                code: code.clone(),
            }
            .publish(&env);
        }

        Ok(())
    }

    /// Remove stored jurisdiction codes for each address in `addresses`.
    pub fn remove_jurisdiction_multiple(
        env: Env,
        issuer: Address,
        addresses: Vec<Address>,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        for address in addresses.iter() {
            env.storage()
                .persistent()
                .remove(&DataKey::Jurisdiction(address.clone()));
            JurisdictionRemoved { address }.publish(&env);
        }
        Ok(())
    }

    /// Returns the jurisdiction code attached to `address`, if any.
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        let key = DataKey::Jurisdiction(address);
        let code: Option<String> = env.storage().persistent().get(&key);
        if code.is_some() {
            Self::extend_jurisdiction_ttl(&env, &key);
        }
        code
    }

    /// Returns `true` if `address` has a jurisdiction code that appears in
    /// `allowed_codes`. Meant to be called by other contracts enforcing a
    /// permitted-jurisdiction policy.
    ///
    /// Returns `Err(EmptyAllowedCodes)` when `allowed_codes` is empty, since
    /// that always produces `false` and is almost certainly a caller mistake.
    pub fn is_permitted_jurisdiction(
        env: Env,
        address: Address,
        allowed_codes: Vec<String>,
    ) -> Result<bool, Error> {
        if allowed_codes.is_empty() {
            return Err(Error::EmptyAllowedCodes);
        }
        Ok(match Self::get_jurisdiction(env, address) {
            Some(code) => allowed_codes.iter().any(|c| c == code),
            None => false,
        })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Upgrade the contract WASM. Issuer-only.
    ///
    /// Uses Soroban's native `update_current_contract_wasm` host function to
    /// swap the contract code behind the same contract ID. All existing
    /// storage (issuer address, jurisdiction flags) is preserved across the
    /// upgrade. The issuer's auth is verified before the upgrade proceeds.
    pub fn upgrade(env: Env, issuer: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    fn require_issuer(env: &Env, issuer: &Address) -> Result<(), Error> {
        issuer.require_auth();
        let stored_issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .ok_or(Error::NotInitialized)?;
        if stored_issuer != *issuer {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }

    /// Checks that `caller` is either the issuer or the compliance officer.
    fn require_compliance_authority(env: &Env, caller: &Address) -> Result<(), Error> {
        caller.require_auth();
        let stored_issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .ok_or(Error::NotInitialized)?;
        if stored_issuer == *caller {
            return Ok(());
        }
        if let Some(officer) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::ComplianceOfficer)
        {
            if officer == *caller {
                return Ok(());
            }
        }
        Err(Error::NotAuthorized)
    }

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            Err(Error::ContractPaused)
        } else {
            Ok(())
        }
    }

    /// Validate that a jurisdiction code is non-empty and contains only
    /// ASCII uppercase letters.  ISO 3166-1 alpha-2 (2 chars) and alpha-3
    /// (3 chars) codes are the primary use case, but the contract does not
    /// enforce length so callers may use longer regional codes.
    fn validate_code(code: &String) -> Result<(), Error> {
        if code.is_empty() {
            return Err(Error::InvalidInput);
        }
        // Iterate over the raw byte representation: each byte must be an
        // ASCII uppercase letter (A–Z).
        let len = code.len() as usize;
        for i in 0..len {
            let b = code.get(i as u32).ok_or(Error::InvalidInput)?;
            if !(b'A'..=b'Z').contains(&(b as u8)) {
                return Err(Error::InvalidInput);
            }
        }
        Ok(())
    }

    fn extend_jurisdiction_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod fuzz;
