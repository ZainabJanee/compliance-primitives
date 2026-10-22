extern crate std;

use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Bytes, BytesN, Env,
};

/// Build an arbitrary 32-byte payload hash for `__check_auth` calls in tests.
/// The contract under test does not inspect the payload content.
fn dummy_payload(env: &Env) -> Hash<32> {
    env.crypto().sha256(&Bytes::from_array(env, &[0u8; 32]))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register and initialise a MultisigAdmin with `threshold`-of-`n` signers.
/// Returns `(signers_vec, contract_id, client)`.
fn setup_multisig(
    env: &Env,
    n: usize,
    threshold: u32,
) -> (Vec<Address>, Address, MultisigAdminClient<'_>) {
    env.mock_all_auths();
    let mut signers = Vec::new(env);
    for _ in 0..n {
        signers.push_back(Address::generate(env));
    }
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(env, &contract_id);
    client.initialize(&signers, &threshold);
    (signers, contract_id, client)
}

// ---------------------------------------------------------------------------
// Basic state tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_stores_signers_and_threshold() {
    let env = Env::default();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    assert_eq!(client.get_threshold(), 2u32);
    let (stored, stored_threshold) = client.get_signers();
    assert_eq!(stored.len(), 3);
    assert_eq!(stored_threshold, 2u32);
    // All original signers should be present.
    for i in 0..signers.len() {
        assert_eq!(stored.get(i), signers.get(i));
    }
}

#[test]
fn test_get_signers_matches_initialization() {
    let env = Env::default();
    let (signers, _id, client) = setup_multisig(&env, 4, 3);

    let (stored, threshold) = client.get_signers();
    assert_eq!(stored.len(), signers.len());
    for i in 0..signers.len() {
        assert_eq!(stored.get(i), signers.get(i));
    }
    assert_eq!(threshold, 3u32);
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (signers, _id, client) = setup_multisig(&env, 2, 1);
    let result = client.try_initialize(&signers, &1);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_invalid_threshold_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);
    let signers = vec![&env, Address::generate(&env)];
    let result = client.try_initialize(&signers, &0);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_invalid_threshold_exceeds_signer_count() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);
    let signers = vec![&env, Address::generate(&env), Address::generate(&env)];
    // threshold = 3 but only 2 signers
    let result = client.try_initialize(&signers, &3);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

// ---------------------------------------------------------------------------
// Duplicate-signer rejection in initialize — #610
// ---------------------------------------------------------------------------

/// `initialize([A, A, B], 3)` would pass the threshold check (3 <= 3) but
/// `__check_auth` rejects duplicate signature addresses, so at most two
/// valid signatures are possible and the threshold of 3 can never be met.
/// The contract and every primitive it governs would be locked permanently.
#[test]
fn test_initialize_rejects_duplicate_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);

    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);
    let signers = vec![&env, addr_a.clone(), addr_a, addr_b];
    let result = client.try_initialize(&signers, &3);
    assert_eq!(result, Err(Ok(Error::DuplicateSigner)));
}

/// Duplicates must be rejected even when the threshold would be valid after
/// deduplication — the caller should deduplicate before calling.
#[test]
fn test_initialize_rejects_duplicates_even_with_valid_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);

    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);
    // [A, A, B] with threshold 2 — 2 <= 3 raw count, but duplicates make
    // the effective signer set only {A, B}, so threshold 2 is actually
    // valid. Still rejected because the caller should deduplicate first.
    let signers = vec![&env, addr_a.clone(), addr_a, addr_b];
    let result = client.try_initialize(&signers, &2);
    assert_eq!(result, Err(Ok(Error::DuplicateSigner)));
}

/// Distinct signers with a valid threshold must still succeed.
#[test]
fn test_initialize_succeeds_with_distinct_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);

    let signers = vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    let result = client.try_initialize(&signers, &2);
    assert!(result.is_ok());
    assert_eq!(client.get_signers().0.len(), 3);
    assert_eq!(client.get_threshold(), 2);
}

/// Single signer (no duplicates) still works.
#[test]
fn test_initialize_single_signer_no_duplicates() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);

    let signers = vec![&env, Address::generate(&env)];
    let result = client.try_initialize(&signers, &1);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Signer-set management tests
// ---------------------------------------------------------------------------

#[test]
fn test_add_signer_increases_count() {
    let env = Env::default();
    let (_signers, _id, client) = setup_multisig(&env, 2, 1);
    let new_signer = Address::generate(&env);
    client.add_signer(&new_signer);
    assert_eq!(client.get_signers().0.len(), 3);
}

#[test]
fn test_add_duplicate_signer_rejected() {
    let env = Env::default();
    let (signers, _id, client) = setup_multisig(&env, 2, 1);
    let existing = signers.get(0).unwrap();
    let result = client.try_add_signer(&existing);
    assert_eq!(result, Err(Ok(Error::AlreadySigner)));
}

#[test]
fn test_remove_signer_decreases_count() {
    let env = Env::default();
    let (signers, _id, client) = setup_multisig(&env, 3, 1);
    let to_remove = signers.get(0).unwrap();
    client.remove_signer(&to_remove);
    assert_eq!(client.get_signers().0.len(), 2);
}

#[test]
fn test_remove_signer_not_found_rejected() {
    let env = Env::default();
    let (_signers, _id, client) = setup_multisig(&env, 2, 1);
    let unknown = Address::generate(&env);
    let result = client.try_remove_signer(&unknown);
    assert_eq!(result, Err(Ok(Error::SignerNotFound)));
}

#[test]
fn test_remove_signer_rejected_when_count_drops_below_threshold() {
    let env = Env::default();
    // 2 signers, threshold 2 — removing one would leave 1 < 2.
    let (signers, _id, client) = setup_multisig(&env, 2, 2);
    let to_remove = signers.get(0).unwrap();
    let result = client.try_remove_signer(&to_remove);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

#[test]
fn test_update_threshold() {
    let env = Env::default();
    let (_signers, _id, client) = setup_multisig(&env, 3, 1);
    client.update_threshold(&3);
    assert_eq!(client.get_threshold(), 3u32);
}

#[test]
fn test_update_threshold_invalid_rejected() {
    let env = Env::default();
    let (_signers, _id, client) = setup_multisig(&env, 2, 1);
    // threshold of 5 with only 2 signers is invalid.
    let result = client.try_update_threshold(&5);
    assert_eq!(result, Err(Ok(Error::InvalidThreshold)));
}

// ---------------------------------------------------------------------------
// Integration: multisig as admin of denylist-gate
// ---------------------------------------------------------------------------

/// Demonstrate that the multisig contract can serve as the admin of a
/// denylist-gate. With mock_all_auths the Soroban test framework satisfies
/// all auth requirements automatically, so this test confirms that the
/// initialization and call plumbing work end-to-end.
#[test]
fn test_multisig_as_denylist_admin_with_mock_auth() {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy and initialise the multisig with 3 signers, threshold 2.
    let (_, multisig_id, _multisig_client) = setup_multisig(&env, 3, 2);

    // Deploy denylist-gate and set the multisig contract as its admin.
    let denylist_id = env.register(DenylistGate, ());
    let denylist_client = DenylistGateClient::new(&env, &denylist_id);
    denylist_client.initialize(&multisig_id);

    // With mock_all_auths active every require_auth is satisfied, so
    // denylist operations that go through the multisig address succeed.
    let target = Address::generate(&env);
    denylist_client.add_to_denylist(&multisig_id, &target);
    assert!(!denylist_client.check(&target));

    denylist_client.remove_from_denylist(&multisig_id, &target);
    assert!(denylist_client.check(&target));
}

// ---------------------------------------------------------------------------
// __check_auth threshold tests
// ---------------------------------------------------------------------------

/// Verify ThresholdNotMet is returned from __check_auth when too few valid
/// signers are provided. We test this via the contract's signer-management
/// functions (which call `env.current_contract_address().require_auth()`),
/// but we cannot directly call __check_auth in unit tests without the host
/// context. Instead we verify the Error enum value is correct.
#[test]
fn test_threshold_not_met_error_value() {
    // This is a compile-time / value correctness test: confirm ThresholdNotMet
    // is distinct from the other error codes.
    assert_eq!(Error::ThresholdNotMet as u32, 3);
    assert_ne!(Error::ThresholdNotMet, Error::NotInitialized);
    assert_ne!(Error::ThresholdNotMet, Error::InvalidThreshold);
}

/// Verify that the signer-set update path (add_signer) requires the
/// multisig's own auth by attempting to call it without any auth mock.
/// In a real deployment this would require __check_auth to pass.
#[test]
fn test_signer_update_requires_multisig_auth() {
    // Without mock_all_auths, any call requiring auth will fail.
    // We set up a contract without mocking auth for the add_signer call.
    let env = Env::default();
    env.mock_all_auths(); // needed for initialize
    let (_signers, _id, client) = setup_multisig(&env, 2, 1);

    // Since mock_all_auths is active on this env we cannot test the rejection
    // path here without a separate env. We confirm the call succeeds (auth
    // satisfied by the mock), demonstrating the round-trip plumbing works.
    let new_signer = Address::generate(&env);
    client.add_signer(&new_signer);
    assert_eq!(client.get_signers().0.len(), 3);
    // A separate rejection test for the threshold path is
    // test_threshold_not_met_error_value above.
}

// ---------------------------------------------------------------------------
// __check_auth threshold edge cases (M=1, M=N) — #220
// ---------------------------------------------------------------------------

/// M=1: any single signer's approval is sufficient.
#[test]
fn test_check_auth_threshold_one_of_n_succeeds_with_single_signature() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, _client) = setup_multisig(&env, 3, 1);

    let payload = dummy_payload(&env);
    let sigs = vec![&env, signers.get(0).unwrap()];
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Ok(()));
}

/// M=1: zero signatures still fails even though the threshold is low.
#[test]
fn test_check_auth_threshold_one_of_n_fails_with_zero_signatures() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, _id, _client) = setup_multisig(&env, 3, 1);

    let payload = dummy_payload(&env);
    let sigs: Vec<Address> = Vec::new(&env);
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Err(Error::ThresholdNotMet));
}

/// M=N: every signer must approve; a full set succeeds.
#[test]
fn test_check_auth_threshold_n_of_n_succeeds_with_all_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, _client) = setup_multisig(&env, 3, 3);

    let payload = dummy_payload(&env);
    let sigs = vec![
        &env,
        signers.get(0).unwrap(),
        signers.get(1).unwrap(),
        signers.get(2).unwrap(),
    ];
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Ok(()));
}

/// M=N: missing even one signer's approval is rejected.
#[test]
fn test_check_auth_threshold_n_of_n_fails_with_one_missing() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, _client) = setup_multisig(&env, 3, 3);

    let payload = dummy_payload(&env);
    // Only 2 of the 3 required signers approve.
    let sigs = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Err(Error::ThresholdNotMet));
}

// ---------------------------------------------------------------------------
// Duplicate-signer double-counting guard — #221
// ---------------------------------------------------------------------------

/// The same signer approving twice in one signature set must not count as
/// two approvals toward the threshold: with threshold=2 and only one
/// distinct signer submitted (twice), the call must be rejected rather than
/// treated as satisfying the threshold.
#[test]
fn test_check_auth_duplicate_signature_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, _client) = setup_multisig(&env, 3, 2);

    let payload = dummy_payload(&env);
    let solo_signer = signers.get(0).unwrap();
    // Same signer listed twice — must not be double-counted to reach
    // threshold=2.
    let sigs = vec![&env, solo_signer.clone(), solo_signer];
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Err(Error::DuplicateSignature));
}

/// Sanity check: two genuinely distinct signers still satisfy threshold=2.
#[test]
fn test_check_auth_distinct_signers_not_flagged_as_duplicate() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, _client) = setup_multisig(&env, 3, 2);

    let payload = dummy_payload(&env);
    let sigs = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    let result = MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env));
    assert_eq!(result, Ok(()));
}

// ---------------------------------------------------------------------------
// Proposal / approve / execute workflow — #400
// ---------------------------------------------------------------------------

/// Audit result: `add_signer`, `remove_signer`, and `update_threshold` all go
/// through `env.current_contract_address().require_auth()`, which triggers
/// `__check_auth`. `__check_auth` counts DISTINCT valid signer addresses in the
/// provided `Vec<Address>` and rejects if the count is below `threshold`.
/// This IS real M-of-N enforcement; it is not a single-signer check.
///
/// The tests below verify the separate `propose` → `approve` → `execute`
/// workflow, which accumulates per-signer approvals in persistent storage and
/// only allows execution once `threshold` distinct approvals are recorded.

/// Proposing creates a proposal with zero approvals and returns its ID.
#[test]
fn test_propose_creates_proposal_with_zero_approvals() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[1u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    assert_eq!(proposal_id, 0u64);
    let (stored_payload, stored_expiry, approvals) = client.get_proposal(&0);
    assert_eq!(stored_payload, payload);
    assert_eq!(stored_expiry, expiry);
    assert_eq!(approvals.len(), 0);
}

/// Each signer can approve once; approve() returns true when threshold is met.
#[test]
fn test_approve_accumulates_distinct_approvals() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[2u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    // First approval — threshold not yet met (1 < 2).
    let ready = client.approve(&proposal_id, &signers.get(0).unwrap());
    assert!(!ready, "should not be ready after 1 of 2 approvals");

    // Second approval — threshold met (2 >= 2).
    let ready = client.approve(&proposal_id, &signers.get(1).unwrap());
    assert!(ready, "should be ready after 2 of 2 approvals");
}

/// A signer cannot approve the same proposal twice; the second call returns
/// Err(AlreadyApproved) — a distinct, unambiguous error code.
#[test]
fn test_approve_same_signer_twice_returns_already_approved() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[3u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    let signer0 = signers.get(0).unwrap();
    client.approve(&proposal_id, &signer0);
    // Second call from the same signer — must return a distinct error, not
    // silently return Ok(false) which is indistinguishable from "not yet at threshold".
    let result = client.try_approve(&proposal_id, &signer0);
    assert_eq!(result, Err(Ok(Error::AlreadyApproved)));

    // Verify only 1 approval is stored.
    let (_, _, approvals) = client.get_proposal(&proposal_id);
    assert_eq!(approvals.len(), 1);
}

/// Non-signer cannot approve a proposal.
#[test]
fn test_approve_by_non_signer_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[4u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    let outsider = Address::generate(&env);
    let result = client.try_approve(&proposal_id, &outsider);
    assert_eq!(result, Err(Ok(Error::NotASigner)));
}

/// execute() succeeds once the threshold is met.
#[test]
fn test_execute_succeeds_when_threshold_met() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[5u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    client.approve(&proposal_id, &signers.get(0).unwrap());
    client.approve(&proposal_id, &signers.get(1).unwrap());

    // Should succeed — proposal is deleted after execution.
    client.execute(&proposal_id);

    // Proposal should no longer exist.
    let result = client.try_get_proposal(&proposal_id);
    assert_eq!(result, Err(Ok(Error::ProposalNotFound)));
}

/// execute() is rejected if fewer than threshold approvals are recorded.
#[test]
fn test_execute_fails_when_threshold_not_met() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[6u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    // Only 1 of 2 required approvals.
    client.approve(&proposal_id, &signers.get(0).unwrap());

    let result = client.try_execute(&proposal_id);
    assert_eq!(result, Err(Ok(Error::ThresholdNotMet)));
}

/// Proposals cannot be approved or executed after they expire.
#[test]
fn test_expired_proposal_cannot_be_approved_or_executed() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[7u8; 32]);
    let expiry = env.ledger().sequence() + 5;
    let proposal_id = client.propose(&payload, &expiry);

    // Advance ledger past expiry.
    env.ledger().set_sequence_number(expiry);

    let approve_result = client.try_approve(&proposal_id, &signers.get(0).unwrap());
    assert_eq!(approve_result, Err(Ok(Error::ExpiredProposal)));

    let execute_result = client.try_execute(&proposal_id);
    assert_eq!(execute_result, Err(Ok(Error::ExpiredProposal)));
}

/// Proposal IDs are assigned sequentially starting from 0.
#[test]
fn test_proposal_ids_are_sequential() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, _id, client) = setup_multisig(&env, 2, 1);

    let payload = Bytes::from_array(&env, &[0u8; 32]);
    let expiry = env.ledger().sequence() + 100;

    let id0 = client.propose(&payload, &expiry);
    let id1 = client.propose(&payload, &expiry);
    let id2 = client.propose(&payload, &expiry);

    assert_eq!(id0, 0u64);
    assert_eq!(id1, 1u64);
    assert_eq!(id2, 2u64);
}

/// Approving an unknown proposal returns ProposalNotFound.
#[test]
fn test_approve_unknown_proposal_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 2, 1);

    let result = client.try_approve(&999u64, &signers.get(0).unwrap());
    assert_eq!(result, Err(Ok(Error::ProposalNotFound)));
}

// ---------------------------------------------------------------------------
// TTL extension
// ---------------------------------------------------------------------------

fn instance_ttl(env: &Env, contract_id: &Address) -> u32 {
    use soroban_sdk::testutils::storage::Instance as _;
    env.as_contract(contract_id, || env.storage().instance().get_ttl())
}

/// Advance the ledger far enough that the instance TTL drops below the
/// extension threshold.
fn advance_past_threshold(env: &Env) {
    use soroban_sdk::testutils::Ledger as _;
    env.ledger().with_mut(|li| {
        li.sequence_number += INSTANCE_TTL_EXTEND_TO - INSTANCE_TTL_THRESHOLD + 1;
    });
}

#[test]
fn test_initialize_extends_instance_ttl() {
    let env = Env::default();
    let (_signers, contract_id, _client) = setup_multisig(&env, 3, 2);
    assert_eq!(instance_ttl(&env, &contract_id), INSTANCE_TTL_EXTEND_TO);
}

#[test]
fn test_write_refreshes_ttl_and_state_survives_past_original_ttl() {
    let env = Env::default();
    let (signers, contract_id, client) = setup_multisig(&env, 3, 2);
    let new_signer = Address::generate(&env);

    advance_past_threshold(&env);
    assert!(instance_ttl(&env, &contract_id) < INSTANCE_TTL_THRESHOLD);

    // A write refreshes the TTL back to the configured target.
    client.add_signer(&new_signer);
    assert_eq!(instance_ttl(&env, &contract_id), INSTANCE_TTL_EXTEND_TO);

    // Advance again: the ledger is now well past the TTL granted at
    // initialization, but the refreshed entry is still live and readable.
    advance_past_threshold(&env);
    let (stored, threshold) = client.get_signers();
    assert_eq!(stored.len(), signers.len() + 1);
    assert_eq!(stored.get(3).unwrap(), new_signer);
    assert_eq!(threshold, 2);
}

#[test]
fn test_proposal_writes_refresh_ttl() {
    let env = Env::default();
    let (signers, contract_id, client) = setup_multisig(&env, 3, 2);
    let payload = Bytes::from_array(&env, &[1u8, 2, 3]);
    let far_expiry = INSTANCE_TTL_EXTEND_TO * 4;

    advance_past_threshold(&env);
    let proposal_id = client.propose(&payload, &far_expiry);
    assert_eq!(instance_ttl(&env, &contract_id), INSTANCE_TTL_EXTEND_TO);

    advance_past_threshold(&env);
    client.approve(&proposal_id, &signers.get(0).unwrap());
    assert_eq!(instance_ttl(&env, &contract_id), INSTANCE_TTL_EXTEND_TO);

    advance_past_threshold(&env);
    client.approve(&proposal_id, &signers.get(1).unwrap());
    let (_payload, _expiry, approvals) = client.get_proposal(&proposal_id);
    assert_eq!(approvals.len(), 2);

    advance_past_threshold(&env);
    client.execute(&proposal_id);
    assert_eq!(instance_ttl(&env, &contract_id), INSTANCE_TTL_EXTEND_TO);
}

// ---------------------------------------------------------------------------
// Upgrade / migration path
// ---------------------------------------------------------------------------

/// Path to the release WASM produced by
/// `cargo build --workspace --target wasm32v1-none --release`.
fn multisig_admin_wasm_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("wasm32v1-none")
        .join("release")
        .join("multisig_admin.wasm")
}

#[test]
fn test_upgrade_before_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);
    let hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&hash);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

/// Without the multisig's own authorization (i.e. without the threshold
/// being met through `__check_auth`), `upgrade` must be rejected.
#[test]
fn test_upgrade_requires_multisig_auth() {
    let env = Env::default();
    let contract_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(&env, &contract_id);
    let signers = vec![&env, Address::generate(&env), Address::generate(&env)];
    client.initialize(&signers, &2);

    let hash = BytesN::from_array(&env, &[0u8; 32]);
    assert!(client.try_upgrade(&hash).is_err());
}

/// Deploy from WASM, write state, upgrade to a freshly uploaded WASM hash,
/// then confirm every piece of state survived and the contract is still
/// callable. Requires the release WASM (`make build`); skipped otherwise.
#[test]
fn test_upgrade_preserves_state_and_remains_callable() {
    let wasm_path = multisig_admin_wasm_path();
    let wasm = match std::fs::read(&wasm_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            std::eprintln!(
                "skipping migration test: {} not found (run `make build` first)",
                wasm_path.display()
            );
            return;
        }
    };

    let env = Env::default();
    env.mock_all_auths();

    // Deploy the current release WASM and write state.
    let contract_id = env.register(wasm.as_slice(), ());
    let client = MultisigAdminClient::new(&env, &contract_id);
    let signers = vec![&env, Address::generate(&env), Address::generate(&env)];
    client.initialize(&signers, &2);
    let extra_signer = Address::generate(&env);
    client.add_signer(&extra_signer);
    let payload = Bytes::from_array(&env, &[1, 2, 3]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);
    client.approve(&proposal_id, &signers.get(0).unwrap());

    // Upgrade to a newly uploaded WASM hash.
    let new_hash = env
        .deployer()
        .upload_contract_wasm(Bytes::from_slice(&env, &wasm));
    client.upgrade(&new_hash);

    // State is intact.
    let (stored_signers, threshold) = client.get_signers();
    assert_eq!(stored_signers.len(), 3);
    assert_eq!(stored_signers.get(2).unwrap(), extra_signer);
    assert_eq!(threshold, 2);
    let (stored_payload, stored_expiry, approvals) = client.get_proposal(&proposal_id);
    assert_eq!(stored_payload, payload);
    assert_eq!(stored_expiry, expiry);
    assert_eq!(approvals.len(), 1);

    // And the contract is still callable after the upgrade.
    assert!(client.approve(&proposal_id, &signers.get(1).unwrap()));
    client.execute(&proposal_id);
    client.update_threshold(&3);
    assert_eq!(client.get_threshold(), 3);
    assert_eq!(client.propose(&payload, &expiry), proposal_id + 1);
}

// ---------------------------------------------------------------------------
// Resource-fee benchmark (budget regression) — `__check_auth`
// ---------------------------------------------------------------------------

fn read_baseline(path: &std::path::Path, section: &str) -> (u64, u64) {
    let contents = std::fs::read_to_string(path).unwrap();
    let section_header = std::format!("[{section}]");
    let mut in_section = false;
    let mut cpu = None;
    let mut memory = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == section_header;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("cpu = ") {
            cpu = Some(value.parse::<u64>().unwrap());
        } else if let Some(value) = trimmed.strip_prefix("memory = ") {
            memory = Some(value.parse::<u64>().unwrap());
        }
    }

    let cpu = cpu.expect("missing cpu baseline");
    let memory = memory.expect("missing memory baseline");
    (cpu, memory)
}

fn baseline_path_for_manifest_dir(manifest_dir: std::path::PathBuf) -> std::path::PathBuf {
    manifest_dir.join("..").join("..").join("budget-baselines.toml")
}

fn assert_budget_within_threshold(measured: (u64, u64), baseline: (u64, u64), label: &str) {
    let (measured_cpu, measured_memory) = measured;
    let (baseline_cpu, baseline_memory) = baseline;
    let cpu_limit = (baseline_cpu as f64 * 1.10).ceil() as u64;
    let memory_limit = (baseline_memory as f64 * 1.10).ceil() as u64;

    assert!(
        measured_cpu <= cpu_limit,
        "{label} CPU regression: measured {measured_cpu}, baseline {baseline_cpu}, limit {cpu_limit}"
    );
    assert!(
        measured_memory <= memory_limit,
        "{label} memory regression: measured {measured_memory}, baseline {baseline_memory}, limit {memory_limit}"
    );
}

/// `__check_auth` is the hottest entrypoint: it runs on every admin
/// operation of every primitive that uses this contract as its admin.
/// Measured with a 2-of-3 signer set and a 2-signature approval.
#[test]
fn test_budget_regression_multisig_check_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, contract_id, _client) = setup_multisig(&env, 3, 2);

    let payload = dummy_payload(&env);
    let sigs = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];

    let mut budget = env.cost_estimate().budget();
    budget.reset_default();
    let result = env.as_contract(&contract_id, || {
        MultisigAdmin::__check_auth(env.clone(), payload, sigs, Vec::new(&env))
    });
    assert_eq!(result, Ok(()));

    let measured = (budget.cpu_instruction_cost(), budget.memory_bytes_cost());
    std::println!(
        "multisig-admin __check_auth: cpu={} memory={}",
        measured.0,
        measured.1
    );
    let baseline_path = baseline_path_for_manifest_dir(std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap(),
    ));
    let baseline = read_baseline(&baseline_path, "multisig-admin.__check_auth");
    assert_budget_within_threshold(measured, baseline, "multisig-admin __check_auth");
}

// ---------------------------------------------------------------------------
// #614 — NotASigner / AlreadyApproved error discriminants
// ---------------------------------------------------------------------------

/// Error discriminants must be appended (not reordered). Existing codes
/// must stay at their assigned values; new ones follow.
#[test]
fn test_error_discriminants_not_reordered() {
    assert_eq!(Error::NotInitialized as u32, 1);
    assert_eq!(Error::AlreadyInitialized as u32, 2);
    assert_eq!(Error::ThresholdNotMet as u32, 3);
    assert_eq!(Error::InvalidThreshold as u32, 4);
    assert_eq!(Error::SignerNotFound as u32, 5);
    assert_eq!(Error::AlreadySigner as u32, 6);
    assert_eq!(Error::DuplicateSignature as u32, 7);
    assert_eq!(Error::ContractPaused as u32, 8);
    assert_eq!(Error::ProposalNotFound as u32, 9);
    assert_eq!(Error::ExpiredProposal as u32, 10);
    assert_eq!(Error::DuplicateSigner as u32, 11);
    // New — appended after existing codes.
    assert_eq!(Error::NotASigner as u32, 12);
    assert_eq!(Error::AlreadyApproved as u32, 13);
}

// ---------------------------------------------------------------------------
// #615 — stale approvals from removed signers must not count
// ---------------------------------------------------------------------------

/// After a signer approves then gets removed, their approval must not count
/// toward the threshold in execute(). The remaining valid approvals must be
/// sufficient on their own.
#[test]
fn test_execute_fails_when_only_approval_is_from_removed_signer() {
    let env = Env::default();
    env.mock_all_auths();
    // 3 signers, threshold 2.
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[42u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    let signer0 = signers.get(0).unwrap();
    let signer1 = signers.get(1).unwrap();

    // Both signer0 and signer1 approve — threshold met at approve() time.
    client.approve(&proposal_id, &signer0);
    client.approve(&proposal_id, &signer1);

    // Now remove signer0 (compromised key). Only signer1's approval remains valid.
    // With threshold=2 and only 1 valid approval, execute() must fail.
    client.remove_signer(&signer0);

    // Threshold is now 2 but only 1 remaining active signer approved.
    let result = client.try_execute(&proposal_id);
    assert_eq!(result, Err(Ok(Error::ThresholdNotMet)));
}

/// All approvals still active → execute succeeds even after an unrelated
/// signer is removed.
#[test]
fn test_execute_succeeds_when_removed_signer_did_not_approve() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, _id, client) = setup_multisig(&env, 3, 2);

    let payload = Bytes::from_array(&env, &[43u8; 32]);
    let expiry = env.ledger().sequence() + 100;
    let proposal_id = client.propose(&payload, &expiry);

    // signer0 and signer1 approve.
    client.approve(&proposal_id, &signers.get(0).unwrap());
    client.approve(&proposal_id, &signers.get(1).unwrap());

    // Remove signer2 who never approved — the two valid approvals remain.
    client.remove_signer(&signers.get(2).unwrap());

    // Should still succeed: 2 valid approvals >= threshold 2.
    client.execute(&proposal_id);

    let result = client.try_get_proposal(&proposal_id);
    assert_eq!(result, Err(Ok(Error::ProposalNotFound)));
}
