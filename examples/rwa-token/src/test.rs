use super::*;
use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use circuit_breaker::{CircuitBreaker, CircuitBreakerClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

struct Fixture<'a> {
    admin: Address,
    issuer: Address,
    allowlist_id: Address,
    gate_id: Address,
    jurisdiction_id: Address,
    breaker_id: Address,
    breaker_admin: Address,
    token: RwaTokenClient<'a>,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let issuer = Address::generate(env);
    let breaker_admin = Address::generate(env);

    // Underlying SEP-41 placeholder — rwa-token only uses is_allowed().
    let underlying = Address::generate(env);
    let allowlist_id = env.register(AllowlistToken, ());
    AllowlistTokenClient::new(env, &allowlist_id).initialize(&admin, &underlying);

    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&admin);

    let jurisdiction_id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &jurisdiction_id).initialize(&issuer);

    let breaker_id = env.register(CircuitBreaker, ());
    CircuitBreakerClient::new(env, &breaker_id).initialize(&breaker_admin);

    let allowed_codes = vec![
        env,
        String::from_str(env, "US"),
        String::from_str(env, "CA"),
    ];
    let token_id = env.register(RwaToken, ());
    let token = RwaTokenClient::new(env, &token_id);
    token.initialize(
        &allowlist_id,
        &gate_id,
        &jurisdiction_id,
        &breaker_id,
        &allowed_codes,
    );

    Fixture {
        admin,
        issuer,
        allowlist_id,
        gate_id,
        jurisdiction_id,
        breaker_id,
        breaker_admin,
        token,
    }
}

fn onboard(env: &Env, fx: &Fixture<'_>, who: &Address, code: &str) {
    AllowlistTokenClient::new(env, &fx.allowlist_id).add_to_allowlist(&fx.admin, who, &None);
    JurisdictionFlagClient::new(env, &fx.jurisdiction_id).set_jurisdiction(
        &fx.issuer,
        who,
        &String::from_str(env, code),
    );
}

#[test]
fn test_transfer_succeeds_when_all_checks_pass() {
    let env = Env::default();
    let fx = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    onboard(&env, &fx, &alice, "US");
    onboard(&env, &fx, &bob, "CA");

    fx.token.mint(&alice, &1_000);
    fx.token.transfer(&alice, &bob, &400);

    assert_eq!(fx.token.balance(&alice), 600);
    assert_eq!(fx.token.balance(&bob), 400);
}

#[test]
fn test_transfer_blocked_when_not_allowlisted() {
    let env = Env::default();
    let fx = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    // Alice fully onboarded; bob has jurisdiction but is NOT allowlisted.
    onboard(&env, &fx, &alice, "US");
    JurisdictionFlagClient::new(&env, &fx.jurisdiction_id).set_jurisdiction(
        &fx.issuer,
        &bob,
        &String::from_str(&env, "CA"),
    );

    fx.token.mint(&alice, &1_000);
    let result = fx.token.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::NotAllowlisted)));
    assert_eq!(fx.token.balance(&alice), 1_000);
}

#[test]
fn test_transfer_blocked_when_denied_by_gate() {
    let env = Env::default();
    let fx = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    onboard(&env, &fx, &alice, "US");
    onboard(&env, &fx, &bob, "CA");

    DenylistGateClient::new(&env, &fx.gate_id).add_to_denylist(&fx.admin, &alice);

    fx.token.mint(&alice, &1_000);
    let result = fx.token.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(fx.token.balance(&alice), 1_000);
}

#[test]
fn test_transfer_blocked_when_jurisdiction_not_permitted() {
    let env = Env::default();
    let fx = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    onboard(&env, &fx, &alice, "US");
    // Bob allowlisted but flagged IR, which is outside allowed_codes (US, CA).
    AllowlistTokenClient::new(&env, &fx.allowlist_id).add_to_allowlist(&fx.admin, &bob, &None);
    JurisdictionFlagClient::new(&env, &fx.jurisdiction_id).set_jurisdiction(
        &fx.issuer,
        &bob,
        &String::from_str(&env, "IR"),
    );

    fx.token.mint(&alice, &1_000);
    let result = fx.token.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::JurisdictionNotPermitted)));
    assert_eq!(fx.token.balance(&alice), 1_000);
}

#[test]
fn test_transfer_blocked_when_circuit_breaker_frozen() {
    let env = Env::default();
    let fx = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    onboard(&env, &fx, &alice, "US");
    onboard(&env, &fx, &bob, "CA");

    // Freeze the circuit breaker — all transfers must halt immediately.
    CircuitBreakerClient::new(&env, &fx.breaker_id).freeze(&fx.breaker_admin, &None);

    fx.token.mint(&alice, &1_000);
    let result = fx.token.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::CircuitBreakerFrozen)));
    // Balance unchanged — transfer was blocked.
    assert_eq!(fx.token.balance(&alice), 1_000);
    assert_eq!(fx.token.balance(&bob), 0);
}
