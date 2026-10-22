//! Proves `policy-engine` is a drop-in simplification for the manually-wired
//! denylist + jurisdiction checks exercised in `test.rs`: same inputs, same
//! pass/fail outcome, but routed through a single `evaluate` call instead of
//! calling each primitive individually.
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use policy_engine::{CheckKind, CombineOp, PolicyEngine, PolicyEngineClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, String};

struct EngineSetup<'a> {
    denylist_admin: Address,
    issuer: Address,
    denylist_gate: DenylistGateClient<'a>,
    jurisdiction_flag: JurisdictionFlagClient<'a>,
    engine: PolicyEngineClient<'a>,
}

fn setup_engine(env: &Env) -> EngineSetup {
    env.mock_all_auths();

    let denylist_admin = Address::generate(env);
    let issuer = Address::generate(env);
    let engine_admin = Address::generate(env);

    let denylist_gate_id = env.register(DenylistGate, ());
    let denylist_gate = DenylistGateClient::new(env, &denylist_gate_id);
    denylist_gate.initialize(&denylist_admin);

    let jurisdiction_flag_id = env.register(JurisdictionFlag, ());
    let jurisdiction_flag = JurisdictionFlagClient::new(env, &jurisdiction_flag_id);
    jurisdiction_flag.initialize(&issuer);

    let engine_id = env.register(PolicyEngine, ());
    let engine = PolicyEngineClient::new(env, &engine_id);
    engine.initialize(&engine_admin, &CombineOp::All, &None);

    let usa_code = String::from_str(env, "US");
    engine.add_check(&engine_admin, &CheckKind::Denylist(policy_engine::DenylistCheck { contract: denylist_gate_id }));
    engine.add_check(
        &engine_admin,
        &CheckKind::Jurisdiction(policy_engine::JurisdictionCheck { contract: jurisdiction_flag_id, allowed_codes: vec![env, usa_code] }),
    );

    EngineSetup { denylist_admin, issuer, denylist_gate, jurisdiction_flag, engine }
}

#[test]
fn test_policy_engine_matches_manual_wiring_when_all_pass() {
    let env = Env::default();
    let setup = setup_engine(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let usa_code = String::from_str(&env, "US");

    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Manual wiring: both checks pass independently.
    assert!(setup.denylist_gate.check(&alice) && setup.denylist_gate.check(&bob));
    let permitted = vec![&env, usa_code];
    assert!(
        setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted)
            && setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted)
    );

    // policy-engine: identical outcome via a single evaluate() call.
    assert!(setup.engine.evaluate(&alice, &bob));
}

#[test]
fn test_policy_engine_matches_manual_wiring_when_blocked_by_denylist() {
    let env = Env::default();
    let setup = setup_engine(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let usa_code = String::from_str(&env, "US");

    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &alice);

    // Manual wiring: denylist check fails for alice.
    assert!(!setup.denylist_gate.check(&alice));

    // policy-engine: identical (blocked) outcome.
    assert!(!setup.engine.evaluate(&alice, &bob));
}

#[test]
fn test_policy_engine_matches_manual_wiring_when_blocked_by_jurisdiction() {
    let env = Env::default();
    let setup = setup_engine(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let usa_code = String::from_str(&env, "US");

    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    // bob has no jurisdiction set.

    let permitted = vec![&env, usa_code];
    // Manual wiring: jurisdiction check fails for bob.
    assert!(!setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted));

    // policy-engine: identical (blocked) outcome.
    assert!(!setup.engine.evaluate(&alice, &bob));
}
