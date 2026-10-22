//! `multisig-audit-trail` demonstrates how to use `multisig-admin` proposals
//! in conjunction with `audit-log` to create a complete governance audit trail.
//!
//! ## Governance Flow
//!
//! 1. An authorized signer **proposes** a governance action (e.g., adding a
//!    new signer, updating a denylist). The proposal is created with an
//!    expiry ledger sequence.
//! 2. Other signers **approve** the proposal. Each approval is logged to the
//!    audit-log as a "proposal_approved" event.
//! 3. Once the threshold is met, the proposal is **executed**. This logs an
//!    "proposal_executed" event.
//! 4. If a proposal expires before execution, any attempt to approve or execute
//!    it returns an error. The audit-log maintains the complete approval
//!    history up to that point.
//!
//! ## Why audit-log?
//!
//! Off-chain governance dashboards can query the audit-log to show:
//! - Who proposed what and when
//! - Who approved (or rejected) it
//! - When it was executed
//! - Which proposals expired without execution
//!
//! This creates an immutable, on-chain proof of governance decisions — critical
//! for regulatory compliance and post-incident forensics.
//!
//! ## Integration Pattern
//!
//! This example is not a contract itself; rather, it shows the pattern for
//! wrapping calls to `multisig-admin` with calls to `audit-log.record()`. A
//! real governance contract would follow this pattern, forwarding governance
//! events to a known audit-log instance.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, String, Symbol};

// Trait definition for cross-contract calls to multisig-admin
mod multisig_trait {
    use soroban_sdk::{contractclient, Address, Bytes, Env, Vec};

    #[contractclient(name = "MultisigAdminClient")]
    pub trait MultisigAdmin {
        fn propose(env: Env, payload: Bytes, expiry_ledger: u32) -> u64;
        fn approve(env: Env, proposal_id: u64, approver: Address) -> bool;
        fn execute(env: Env, proposal_id: u64) -> ();
        fn get_proposal(
            env: Env,
            proposal_id: u64,
        ) -> (Bytes, u32, Vec<Address>);
    }
}

// Trait definition for cross-contract calls to audit-log
mod audit_log_trait {
    use soroban_sdk::{contractclient, Address, Env, String, Symbol};

    #[contractclient(name = "AuditLogClient")]
    pub trait AuditLog {
        fn record(
            env: Env,
            source: Address,
            kind: Symbol,
            subject: Address,
            detail: String,
        ) -> ();
    }
}

// Dummy contract serving as the example entry point (non-functional)
#[contract]
pub struct MultisigAuditTrail;

#[contractimpl]
impl MultisigAuditTrail {
    /// This is a documentation example, not a working implementation.
    /// In a real scenario, a governance wrapper would:
    ///
    /// 1. Call `multisig_admin.propose(...)` and get a proposal ID
    /// 2. Forward each approval to the audit-log:
    ///    ```text
    ///    audit_log.record(
    ///        source: governance_contract,
    ///        kind: "proposal_approved",
    ///        subject: approver,
    ///        detail: format!("proposal {}", proposal_id)
    ///    )
    ///    ```
    /// 3. Log execution:
    ///    ```text
    ///    audit_log.record(
    ///        source: governance_contract,
    ///        kind: "proposal_executed",
    ///        subject: Address::try_from_val(&env, &proposal_id).unwrap(),
    ///        detail: "governance action completed"
    ///    )
    ///    ```
    pub fn noop(_env: Env) {}
}

#[cfg(test)]
mod test;
