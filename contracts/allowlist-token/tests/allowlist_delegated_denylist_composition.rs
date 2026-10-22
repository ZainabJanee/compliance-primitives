//! Composition of the delegated-signature onboarding path
//! (`add_to_allowlist_delegated`) with a `denylist-gate` gated token — the
//! realistic setup for an off-chain relayer onboarding users.

use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use denylist_gate_consumer::{ExampleToken, ExampleTokenClient};
use ed25519_dalek::SigningKey;
use soroban_sdk::testutils::ed25519::Sign;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Bytes, BytesN, Env};

struct Composition<'a> {
    gate_admin: Address,
    gate_id: Address,
    allowlist_admin: Address,
    example_token: ExampleTokenClient<'a>,
    allowlist_token: AllowlistTokenClient<'a>,
    signing_key: SigningKey,
}

fn setup_composition(env: &Env) -> Composition<'_> {
    env.mock_all_auths();

    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    let example_token_id = env.register(ExampleToken, ());
    let example_token = ExampleTokenClient::new(env, &example_token_id);
    example_token.initialize(&gate_id);

    let allowlist_admin = Address::generate(env);
    let allowlist_token_id = env.register(AllowlistToken, ());
    let allowlist_token = AllowlistTokenClient::new(env, &allowlist_token_id);
    allowlist_token.initialize(&allowlist_admin, &example_token_id);

    // Off-chain relayer key that may onboard addresses on the admin's behalf.
    let signing_key = SigningKey::from_bytes(&[
        0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
        24, 25, 26, 27, 28, 29, 30, 31,
    ]);
    let pubkey = BytesN::from_array(env, &signing_key.verifying_key().to_bytes());
    allowlist_token.set_delegated_admin_key(&allowlist_admin, &pubkey);

    Composition {
        gate_admin,
        gate_id,
        allowlist_admin,
        example_token,
        allowlist_token,
        signing_key,
    }
}

/// Signs `allowlist-delegated-v1:<target>:add_to_allowlist:<nonce>:<expiry>`.
fn sign_add(env: &Env, key: &SigningKey, target: &Address, nonce: u64, expiry: u64) -> BytesN<64> {
    let mut message = Bytes::new(env);
    message.append(&Bytes::from_slice(env, b"allowlist-delegated-v1:"));
    let target_str = target.to_string().to_string();
    message.append(&Bytes::from_slice(env, target_str.as_bytes()));
    message.push_back(b':');
    message.append(&Bytes::from_slice(env, b"add_to_allowlist"));
    message.push_back(b':');
    message.append(&Bytes::from_slice(env, nonce.to_string().as_bytes()));
    message.push_back(b':');
    message.append(&Bytes::from_slice(env, expiry.to_string().as_bytes()));
    BytesN::from_array(env, &key.sign(&message).unwrap())
}

/// Onboards `address` through the delegated path with the given `nonce`.
fn onboard_delegated(env: &Env, c: &Composition<'_>, address: &Address, nonce: u64) {
    use soroban_sdk::testutils::Ledger as _;
    let expiry = env.ledger().timestamp() + 60;
    let signature = sign_add(env, &c.signing_key, address, nonce, expiry);
    c.allowlist_token
        .add_to_allowlist_delegated(&c.allowlist_admin, address, &nonce, &expiry, &signature);
}

#[test]
fn test_delegated_onboarding_then_transfer_succeeds_through_composed_gate() {
    let env = Env::default();
    let c = setup_composition(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    c.example_token.mint(&alice, &1_000);

    assert!(!c.allowlist_token.is_allowed(&alice));
    onboard_delegated(&env, &c, &alice, 1);
    onboard_delegated(&env, &c, &bob, 2);
    assert!(c.allowlist_token.is_allowed(&alice));
    assert!(c.allowlist_token.is_allowed(&bob));

    let success = c.allowlist_token.transfer(&alice, &bob, &400);
    assert!(success);
    assert_eq!(c.example_token.balance(&alice), 600);
    assert_eq!(c.example_token.balance(&bob), 400);
}

#[test]
fn test_delegated_onboarding_does_not_bypass_denylist_gate() {
    let env = Env::default();
    let c = setup_composition(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    c.example_token.mint(&alice, &1_000);
    onboard_delegated(&env, &c, &alice, 1);
    onboard_delegated(&env, &c, &bob, 2);

    // Bob is allowlisted via the delegated path but denylisted at the gate.
    DenylistGateClient::new(&env, &c.gate_id).add_to_denylist(&c.gate_admin, &bob);

    let result = c.allowlist_token.try_transfer(&alice, &bob, &400);
    assert!(result.is_err());
    assert_eq!(c.example_token.balance(&alice), 1_000);
    assert_eq!(c.example_token.balance(&bob), 0);
}

#[test]
fn test_transfer_blocked_when_recipient_not_onboarded_via_delegation() {
    let env = Env::default();
    let c = setup_composition(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    c.example_token.mint(&alice, &1_000);
    onboard_delegated(&env, &c, &alice, 1);

    let success = c.allowlist_token.transfer(&alice, &bob, &400);
    assert!(!success);
    assert_eq!(c.example_token.balance(&alice), 1_000);
    assert_eq!(c.example_token.balance(&bob), 0);
}
