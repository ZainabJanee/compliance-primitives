use super::*;
use circuit_breaker::{CircuitBreaker, CircuitBreakerClient as BreakerClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use policy_engine::{CheckKind, CombineOp, PolicyEngine, PolicyEngineClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec as svec, Env};

fn setup(env: &Env) -> (Address, Address, Address, Address, Address, Address) {
    env.mock_all_auths();

    // Deploy and initialize circuit breaker
    let breaker_admin = Address::generate(env);
    let breaker_id = env.register(CircuitBreaker, ());
    BreakerClient::new(env, &breaker_id).initialize(&breaker_admin);

    // Deploy and initialize denylist gate
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    // Deploy and initialize policy engine with denylist check
    let policy_admin = Address::generate(env);
    let policy_id = env.register(PolicyEngine, ());
    let policy_client = PolicyEngineClient::new(env, &policy_id);
    policy_client.initialize(&policy_admin, &CombineOp::All);
    
    // Add denylist check to policy
    policy_client.add_check(
        &policy_admin,
        &CheckKind::Denylist {
            contract: gate_id.clone(),
        },
    );

    (
        breaker_admin,
        breaker_id,
        gate_admin,
        gate_id,
        policy_admin,
        policy_id,
    )
}

#[test]
fn test_transfer_passes_when_breaker_unfrozen_and_policy_passes() {
    let env = Env::default();
    let (_breaker_admin, breaker_id, _gate_admin, _gate_id, _policy_admin, policy_id) =
        setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Ok(true));
}

#[test]
fn test_transfer_fails_when_breaker_frozen() {
    let env = Env::default();
    let (breaker_admin, breaker_id, _gate_admin, _gate_id, _policy_admin, policy_id) =
        setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Freeze the breaker
    BreakerClient::new(&env, &breaker_id).freeze(&breaker_admin, &None);

    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Err(Error::CircuitBreakerFrozen));
}

#[test]
fn test_transfer_recovers_after_unfreeze() {
    let env = Env::default();
    let (breaker_admin, breaker_id, _gate_admin, _gate_id, _policy_admin, policy_id) =
        setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Freeze the breaker
    BreakerClient::new(&env, &breaker_id).freeze(&breaker_admin, &None);
    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Err(Error::CircuitBreakerFrozen));

    // Unfreeze the breaker
    BreakerClient::new(&env, &breaker_id).unfreeze(&breaker_admin);
    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Ok(true));
}

#[test]
fn test_policy_violation_when_address_denied() {
    let env = Env::default();
    let (_breaker_admin, breaker_id, gate_admin, gate_id, _policy_admin, policy_id) =
        setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add bob to denylist
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Err(Error::PolicyViolation));
}

#[test]
fn test_breaker_takes_precedence_over_policy() {
    let env = Env::default();
    let (breaker_admin, breaker_id, gate_admin, gate_id, _policy_admin, policy_id) =
        setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add bob to denylist (policy would fail)
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    // Freeze the breaker
    BreakerClient::new(&env, &breaker_id).freeze(&breaker_admin, &None);

    // Should get CircuitBreakerFrozen error, not PolicyViolation
    let result = GatedTransferContract::check_transfer(&env, &breaker_id, &policy_id, &alice, &bob);
    assert_eq!(result, Err(Error::CircuitBreakerFrozen));
}
