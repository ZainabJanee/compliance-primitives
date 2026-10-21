#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String,
};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Frozen,
    PendingAdmin,
}

/// Version + admin metadata about this contract instance, for tooling that
/// introspects deployed contracts.
#[contracttype]
#[derive(Clone)]
pub struct Metadata {
    pub version: String,
    pub admin: Address,
}

#[contractevent]
pub struct Frozen {
    #[topic]
    pub admin: Address,
}

#[contractevent]
pub struct Unfrozen {
    #[topic]
    pub admin: Address,
}

/// Emitted when a new admin is proposed via `propose_admin`.
/// The current admin is in the topic, the proposed new admin in `data`.
#[contractevent]
pub struct AdminProposed {
    #[topic]
    pub admin: Address,
    pub new_admin: Address,
}

/// Emitted when a proposed admin accepts the transfer via `accept_admin`.
/// The new (now current) admin is in the topic; the previous admin is in
/// `data` so monitoring can trace the full rotation.
#[contractevent]
pub struct AdminTransferred {
    #[topic]
    pub new_admin: Address,
    pub old_admin: Address,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    NoPendingAdmin = 4,
    PendingAdminMismatch = 5,
}

#[contract]
pub struct CircuitBreaker;

#[contractimpl]
impl CircuitBreaker {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Frozen, &false);
        Ok(())
    }

    pub fn freeze(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Frozen, &true);
        Frozen {
            admin: admin.clone(),
        }
        .publish(&env);
        Ok(())
    }

    pub fn unfreeze(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Frozen, &false);
        Unfrozen {
            admin: admin.clone(),
        }
        .publish(&env);
        Ok(())
    }

    pub fn is_frozen(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Frozen).unwrap_or(false)
    }

    /// Propose a new admin. The current admin remains fully in control
    /// until the proposed admin calls `accept_admin`; this avoids a
    /// single-typo lockout from a direct admin overwrite.
    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
        AdminProposed {
            admin: admin.clone(),
            new_admin: new_admin.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Accept a pending admin transfer. Must be called by the proposed
    /// admin itself.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        new_admin.require_auth();
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(Error::NoPendingAdmin)?;
        if pending_admin != new_admin {
            return Err(Error::PendingAdminMismatch);
        }
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        AdminTransferred {
            new_admin: new_admin.clone(),
            old_admin,
        }
        .publish(&env);
        Ok(())
    }

    /// Cancel a pending admin proposal. Only the current admin can cancel.
    pub fn cancel_admin_proposal(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            return Err(Error::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    /// Returns the pending admin address if one has been proposed and
    /// not yet cancelled or accepted.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Returns metadata about this contract instance, including version and
    /// admin address.
    pub fn metadata(env: Env) -> Result<Metadata, Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        Ok(Metadata {
            version: String::from_str(&env, env!("CARGO_PKG_VERSION")),
            admin,
        })
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
mod test;
