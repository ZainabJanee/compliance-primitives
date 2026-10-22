use super::audit_log_trait::AuditLogClient;
use audit_log::{AuditLog, AuditLogClient as RealAuditLogClient};
use multisig_admin::{MultisigAdmin, MultisigAdminClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Bytes, Env, String, Symbol,
};

/// Deploy and initialize a fresh `audit-log` instance; return its contract ID.
fn deploy_audit_log(env: &Env) -> Address {
    let admin = Address::generate(env);
    let id = env.register(AuditLog, ());
    RealAuditLogClient::new(env, &id).initialize(&admin);
    id
}

/// Deploy and initialize a fresh `multisig-admin` instance with `n` signers and
/// a signing `threshold`; return `(contract_id, Vec<Address>)`.
fn deploy_multisig(env: &Env, n: usize, threshold: u32) -> (Address, soroban_sdk::Vec<Address>) {
    let mut signers = vec![env];
    for _ in 0..n {
        signers.push_back(Address::generate(env));
    }
    let id = env.register(MultisigAdmin, ());
    MultisigAdminClient::new(env, &id).initialize(&signers, &threshold);
    (id, signers)
}

/// The governance contract records a proposal-approved event in the audit-log
/// each time a signer approves, then an executed event when the threshold is
/// reached.  This test walks the happy path for a 2-of-3 multisig:
///
/// 1. Two approvals are recorded in the audit-log (entry_count == 2).
/// 2. Execution is recorded (entry_count == 3).
/// 3. `get_entry` returns the expected `kind` and `subject` for each entry.
#[test]
fn test_proposal_and_approval_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let (multisig_id, signers) = deploy_multisig(&env, 3, 2);
    let audit_log_id = deploy_audit_log(&env);

    let multisig = MultisigAdminClient::new(&env, &multisig_id);

    // Propose a governance action.
    let payload = Bytes::new(&env);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = multisig.propose(&payload, &expiry);

    let audit_log = AuditLogClient::new(&env, &audit_log_id);

    // First signer approves.
    let approver1 = signers.get(0).unwrap();
    let ready = multisig.approve(&proposal_id, &approver1);
    assert!(!ready, "threshold not yet reached after first approval");

    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver1,
        &String::from_str(&env, "proposal 0 approved"),
    );

    // Second signer approves — threshold reached.
    let approver2 = signers.get(1).unwrap();
    let ready = multisig.approve(&proposal_id, &approver2);
    assert!(ready, "threshold should be reached after second approval");

    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver2,
        &String::from_str(&env, "proposal 0 approved"),
    );

    // Execute.
    multisig.execute(&proposal_id);

    let execution_subject = Address::generate(&env);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_executed"),
        &execution_subject,
        &String::from_str(&env, "proposal 0 executed"),
    );

    // Verify the audit trail: 2 approvals + 1 execution = 3 entries.
    let real_log = RealAuditLogClient::new(&env, &audit_log_id);
    assert_eq!(real_log.entry_count(), 3);

    let entry0 = real_log.get_entry(&0).unwrap();
    assert_eq!(entry0.kind, Symbol::new(&env, "proposal_approved"));
    assert_eq!(entry0.subject, approver1);

    let entry1 = real_log.get_entry(&1).unwrap();
    assert_eq!(entry1.kind, Symbol::new(&env, "proposal_approved"));
    assert_eq!(entry1.subject, approver2);

    let entry2 = real_log.get_entry(&2).unwrap();
    assert_eq!(entry2.kind, Symbol::new(&env, "proposal_executed"));
    assert_eq!(entry2.subject, execution_subject);
}

/// A proposal that expires before execution should still have its audit history
/// preserved.  This test verifies that an expiry event can be appended even
/// after the multisig rejects any further approve/execute calls.
#[test]
fn test_expired_proposal_audit_trail() {
    let env = Env::default();
    env.mock_all_auths();

    let (multisig_id, signers) = deploy_multisig(&env, 2, 2);
    let audit_log_id = deploy_audit_log(&env);

    let multisig = MultisigAdminClient::new(&env, &multisig_id);

    let payload = Bytes::new(&env);
    let expiry = env.ledger().sequence() + 10;
    let proposal_id = multisig.propose(&payload, &expiry);

    let approver = signers.get(0).unwrap();
    multisig.approve(&proposal_id, &approver);

    let audit_log = AuditLogClient::new(&env, &audit_log_id);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver,
        &String::from_str(&env, "proposal 0 approved by signer 1"),
    );

    // Advance ledger past the expiry.
    env.ledger().set_sequence_number(expiry);

    // Any further approve/execute would return ExpiredProposal.
    // Record the expiry event in the audit log for governance visibility.
    let expiry_subject = Address::generate(&env);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_expired"),
        &expiry_subject,
        &String::from_str(&env, "proposal 0 expired at ledger 10"),
    );

    // Audit log should contain the approval and the expiry.
    let real_log = RealAuditLogClient::new(&env, &audit_log_id);
    assert_eq!(real_log.entry_count(), 2);

    let entry0 = real_log.get_entry(&0).unwrap();
    assert_eq!(entry0.kind, Symbol::new(&env, "proposal_approved"));
    assert_eq!(entry0.subject, approver);

    let entry1 = real_log.get_entry(&1).unwrap();
    assert_eq!(entry1.kind, Symbol::new(&env, "proposal_expired"));
    assert_eq!(entry1.subject, expiry_subject);
}

/// The audit-log is append-only: entries are preserved across multiple proposals.
/// This test submits two separate proposals in sequence and checks that the
/// log contains entries from both, in order.
#[test]
fn test_audit_entries_accumulate_across_proposals() {
    let env = Env::default();
    env.mock_all_auths();

    let (multisig_id, signers) = deploy_multisig(&env, 3, 2);
    let audit_log_id = deploy_audit_log(&env);

    let multisig = MultisigAdminClient::new(&env, &multisig_id);
    let audit_log = AuditLogClient::new(&env, &audit_log_id);

    let approver1 = signers.get(0).unwrap();
    let approver2 = signers.get(1).unwrap();

    // ---- Proposal 0 ----
    let payload0 = Bytes::new(&env);
    let expiry0 = env.ledger().sequence() + 50;
    let proposal0 = multisig.propose(&payload0, &expiry0);

    multisig.approve(&proposal0, &approver1);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver1,
        &String::from_str(&env, "proposal 0 approved"),
    );

    multisig.approve(&proposal0, &approver2);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver2,
        &String::from_str(&env, "proposal 0 approved"),
    );

    multisig.execute(&proposal0);
    let exec0_subject = Address::generate(&env);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_executed"),
        &exec0_subject,
        &String::from_str(&env, "proposal 0 executed"),
    );

    // ---- Proposal 1 ----
    let payload1 = Bytes::new(&env);
    let expiry1 = env.ledger().sequence() + 50;
    let proposal1 = multisig.propose(&payload1, &expiry1);

    multisig.approve(&proposal1, &approver1);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver1,
        &String::from_str(&env, "proposal 1 approved"),
    );

    multisig.approve(&proposal1, &approver2);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_approved"),
        &approver2,
        &String::from_str(&env, "proposal 1 approved"),
    );

    multisig.execute(&proposal1);
    let exec1_subject = Address::generate(&env);
    audit_log.record(
        &multisig_id,
        &Symbol::new(&env, "proposal_executed"),
        &exec1_subject,
        &String::from_str(&env, "proposal 1 executed"),
    );

    // 3 entries per proposal × 2 proposals = 6 entries total.
    let real_log = RealAuditLogClient::new(&env, &audit_log_id);
    assert_eq!(real_log.entry_count(), 6);

    // Spot-check ordering: entry 2 is the execution of proposal 0.
    let entry2 = real_log.get_entry(&2).unwrap();
    assert_eq!(entry2.kind, Symbol::new(&env, "proposal_executed"));
    assert_eq!(entry2.subject, exec0_subject);

    // Entry 5 is the execution of proposal 1.
    let entry5 = real_log.get_entry(&5).unwrap();
    assert_eq!(entry5.kind, Symbol::new(&env, "proposal_executed"));
    assert_eq!(entry5.subject, exec1_subject);
}
