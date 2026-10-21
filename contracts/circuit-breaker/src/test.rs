extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Env, IntoVal};

fn setup(env: &Env) -> (Address, CircuitBreakerClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register(CircuitBreaker, ());
    let client = CircuitBreakerClient::new(env, &contract_id);
    client.initialize(&admin);
    (admin, client)
}

#[test]
fn test_is_frozen_defaults_to_false() {
    let env = Env::default();
    let (_admin, client) = setup(&env);
    assert!(!client.is_frozen());
}

#[test]
fn test_admin_can_freeze_and_unfreeze() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    client.freeze(&admin);
    assert!(client.is_frozen());

    client.unfreeze(&admin);
    assert!(!client.is_frozen());
}

#[test]
fn test_freeze_and_unfreeze_emit_events_with_admin() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    client.freeze(&admin);
    let freeze_events = env.events().all();
    let (_contract_id, topics, _data) = freeze_events.last().unwrap();
    assert_eq!(
        topics.get_unchecked(0),
        soroban_sdk::Symbol::new(&env, "frozen").into_val(&env)
    );
    assert_eq!(topics.get_unchecked(1), admin.clone().into_val(&env));

    client.unfreeze(&admin);
    let unfreeze_events = env.events().all();
    let (_contract_id, topics, _data) = unfreeze_events.last().unwrap();
    assert_eq!(
        topics.get_unchecked(0),
        soroban_sdk::Symbol::new(&env, "unfrozen").into_val(&env)
    );
    assert_eq!(topics.get_unchecked(1), admin.clone().into_val(&env));
}

#[test]
fn test_non_admin_cannot_freeze_or_unfreeze() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let impostor = Address::generate(&env);

    let freeze_result = client.try_freeze(&impostor);
    assert_eq!(freeze_result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_frozen());

    let unfreeze_result = client.try_unfreeze(&impostor);
    assert_eq!(unfreeze_result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_frozen());

    client.freeze(&admin);
    assert!(client.is_frozen());

    let unfreeze_result = client.try_unfreeze(&impostor);
    assert_eq!(unfreeze_result, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_frozen());
}

#[test]
fn test_is_frozen_before_initialize_returns_false() {
    let env = Env::default();
    let contract_id = env.register(CircuitBreaker, ());
    let client = CircuitBreakerClient::new(&env, &contract_id);
    // Never called `initialize` on this instance. `is_frozen` should
    // return a sensible default (`false`) rather than panicking.
    assert!(!client.is_frozen());
}

#[test]
fn test_double_freeze_is_idempotent() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    // Calling `freeze` twice in a row is a no-op the second time: state
    // stays frozen and the call itself succeeds rather than erroring.
    client.freeze(&admin);
    assert!(client.is_frozen());
    client.freeze(&admin);
    assert!(client.is_frozen());
}

#[test]
fn test_double_unfreeze_is_idempotent() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    // Calling `unfreeze` twice in a row is a no-op the second time: state
    // stays unfrozen and the call itself succeeds rather than erroring.
    client.unfreeze(&admin);
    assert!(!client.is_frozen());
    client.unfreeze(&admin);
    assert!(!client.is_frozen());
}

#[test]
fn test_propose_then_accept_admin_transfers_control() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // Old admin no longer has control.
    let freeze_result = client.try_freeze(&admin);
    assert_eq!(freeze_result, Err(Ok(Error::NotAuthorized)));

    // New admin does.
    client.freeze(&new_admin);
    assert!(client.is_frozen());
}

#[test]
fn test_propose_never_accepted_old_admin_retains_control() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    // `new_admin` never calls accept_admin.

    // Old admin still has control.
    client.freeze(&admin);
    assert!(client.is_frozen());

    // The proposed admin has no control yet.
    let unfreeze_result = client.try_unfreeze(&new_admin);
    assert_eq!(unfreeze_result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_metadata_after_initialization() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    let metadata = client.metadata();
    assert_eq!(metadata.admin, admin);
    assert_eq!(
        metadata.version,
        soroban_sdk::String::from_str(&env, env!("CARGO_PKG_VERSION"))
    );
}

// ---------------------------------------------------------------------------
// Admin proposal events — #608
// ---------------------------------------------------------------------------

#[test]
fn test_propose_admin_emits_event() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);

    let events = env.events().all();
    let (_contract_id, topics, _data) = events.last().unwrap();
    assert_eq!(
        topics.get_unchecked(0),
        soroban_sdk::Symbol::new(&env, "admin_proposed").into_val(&env)
    );
    assert_eq!(topics.get_unchecked(1), admin.clone().into_val(&env));
}

#[test]
fn test_accept_admin_emits_transferred_event() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    let events = env.events().all();
    let (_contract_id, topics, _data) = events.last().unwrap();
    assert_eq!(
        topics.get_unchecked(0),
        soroban_sdk::Symbol::new(&env, "admin_transferred").into_val(&env)
    );
    assert_eq!(topics.get_unchecked(1), new_admin.clone().into_val(&env));
}

// ---------------------------------------------------------------------------
// cancel_admin_proposal — #608
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_admin_proposal_succeeds() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));

    client.cancel_admin_proposal(&admin);
    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_accept_after_cancel_returns_no_pending_admin() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.cancel_admin_proposal(&admin);

    let result = client.try_accept_admin(&new_admin);
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn test_cancel_admin_proposal_no_pending_returns_error() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    let result = client.try_cancel_admin_proposal(&admin);
    assert_eq!(result, Err(Ok(Error::NoPendingAdmin)));
}

#[test]
fn test_cancel_admin_proposal_non_admin_rejected() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);

    let result = client.try_cancel_admin_proposal(&impostor);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // Proposal is still pending.
    assert_eq!(client.get_pending_admin(), Some(new_admin));
}

#[test]
fn test_get_pending_admin_none_when_no_proposal() {
    let env = Env::default();
    let (_admin, client) = setup(&env);

    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_get_pending_admin_returns_proposed() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin));
}

#[test]
fn test_cancel_then_old_admin_retains_control() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.cancel_admin_proposal(&admin);

    // Old admin still has control after cancellation.
    client.freeze(&admin);
    assert!(client.is_frozen());

    // Proposed admin does not have control.
    let result = client.try_unfreeze(&new_admin);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}
