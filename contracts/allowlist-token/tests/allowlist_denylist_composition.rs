use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use denylist_gate_consumer::{ExampleToken, ExampleTokenClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn setup_composition(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    Address,
    ExampleTokenClient<'_>,
    AllowlistTokenClient<'_>,
) {
    env.mock_all_auths();

    // 1. Deploy & initialize DenylistGate
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    // 2. Deploy & initialize ExampleToken (underlying token pointing to DenylistGate)
    let example_token_id = env.register(ExampleToken, ());
    let example_token_client = ExampleTokenClient::new(env, &example_token_id);
    example_token_client.initialize(&gate_id);

    // 3. Deploy & initialize AllowlistToken (wrapping ExampleToken)
    let allowlist_admin = Address::generate(env);
    let allowlist_token_id = env.register(AllowlistToken, ());
    let allowlist_token_client = AllowlistTokenClient::new(env, &allowlist_token_id);
    allowlist_token_client.initialize(&allowlist_admin, &example_token_id);

    (
        gate_admin,
        gate_id,
        allowlist_admin,
        example_token_id,
        allowlist_token_id,
        example_token_client,
        allowlist_token_client,
    )
}

#[test]
fn test_composition_succeeds_when_both_allowlisted_and_clear_on_gate() {
    let env = Env::default();
    let (
        _gate_admin,
        _gate_id,
        allowlist_admin,
        _example_token_id,
        _allowlist_token_id,
        example_token_client,
        allowlist_token_client,
    ) = setup_composition(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Fund alice on underlying token
    example_token_client.mint(&alice, &1_000);

    // Add both alice and bob to the allowlist
    allowlist_token_client.add_to_allowlist(&allowlist_admin, &alice, &None);
    allowlist_token_client.add_to_allowlist(&allowlist_admin, &bob, &None);

    // Perform transfer through AllowlistToken
    let success = allowlist_token_client.transfer(&alice, &bob, &400);
    assert!(success);

    // Verify underlying balances updated
    assert_eq!(example_token_client.balance(&alice), 600);
    assert_eq!(example_token_client.balance(&bob), 400);
}

#[test]
fn test_composition_blocked_at_gate_layer_when_allowlisted_but_denylisted() {
    let env = Env::default();
    let (
        gate_admin,
        gate_id,
        allowlist_admin,
        _example_token_id,
        _allowlist_token_id,
        example_token_client,
        allowlist_token_client,
    ) = setup_composition(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    example_token_client.mint(&alice, &1_000);

    // Both are allowlisted
    allowlist_token_client.add_to_allowlist(&allowlist_admin, &alice, &None);
    allowlist_token_client.add_to_allowlist(&allowlist_admin, &bob, &None);

    // But bob is added to the denylist gate
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    // Attempt transfer through AllowlistToken -> forwards to ExampleToken -> fails at DenylistGate
    let result = allowlist_token_client.try_transfer(&alice, &bob, &400);
    assert!(result.is_err());

    // Balances remain unchanged
    assert_eq!(example_token_client.balance(&alice), 1_000);
    assert_eq!(example_token_client.balance(&bob), 0);
}

#[test]
fn test_composition_blocked_at_allowlist_layer_before_gate_consulted() {
    let env = Env::default();
    let (
        _gate_admin,
        _gate_id,
        allowlist_admin,
        _example_token_id,
        _allowlist_token_id,
        example_token_client,
        allowlist_token_client,
    ) = setup_composition(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    example_token_client.mint(&alice, &1_000);

    // Only alice is allowlisted; bob is NOT allowlisted
    allowlist_token_client.add_to_allowlist(&allowlist_admin, &alice, &None);

    // Transfer through AllowlistToken returns Ok(false) as it blocks before reaching ExampleToken / DenylistGate
    let success = allowlist_token_client.transfer(&alice, &bob, &400);
    assert!(!success);

    // Balances remain unchanged
    assert_eq!(example_token_client.balance(&alice), 1_000);
    assert_eq!(example_token_client.balance(&bob), 0);
}
