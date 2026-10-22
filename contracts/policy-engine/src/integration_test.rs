//! Integration tests: policy-engine chaining all three compliance primitives
//! with AND (All) semantics, plus a smoke test for OR (Any).
//!
//! Scenario: a realistic RWA transfer policy where a transfer is only
//! permitted when the counter-parties are:
//!   1. Present on the allowlist (`allowlist-token.is_allowed`)
//!   2. Not on the denylist (`denylist-gate.check`)
//!   3. In a permitted jurisdiction (`jurisdiction-flag.is_permitted_jurisdiction`)
//!
//! Test matrix
//! ───────────────────────────────────────────────────────────────────────────
//!  test_all_three_pass              — both parties clear all three gates → true
//!  test_fail_allowlist_check        — from is NOT allowlisted → false
//!  test_fail_denylist_check         — from IS denylisted → false
//!  test_fail_jurisdiction_check     — to has a forbidden jurisdiction code → false
//!  test_or_semantics_smoke          — Any: denylist blocks but allowlist passes → true

use crate::{
    AllowlistCheck, CheckKind, CombineOp, DenylistCheck, JurisdictionCheck, PolicyEngine,
    PolicyEngineClient,
};
use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, String};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Jurisdiction code used for parties that should pass the jurisdiction gate.
const PERMITTED_CODE: &str = "US";
/// Jurisdiction code that is deliberately outside the permitted set.
const FORBIDDEN_CODE: &str = "IR";

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Registers and initialises an allowlist-token (pointing to a mock
/// underlying token address that is never actually called in these tests
/// since we only call `is_allowed`, not `transfer`).
fn setup_allowlist(env: &Env, admin: &Address) -> Address {
    let mock_underlying = Address::generate(env);
    let id = env.register(AllowlistToken, ());
    AllowlistTokenClient::new(env, &id).initialize(admin, &mock_underlying);
    id
}

/// Registers and initialises a denylist-gate contract.
fn setup_denylist(env: &Env, admin: &Address) -> Address {
    let id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &id).initialize(admin);
    id
}

/// Registers and initialises a jurisdiction-flag contract.
fn setup_jurisdiction(env: &Env, issuer: &Address) -> Address {
    let id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &id).initialize(issuer);
    id
}

/// Builds and configures a policy-engine with All semantics, wired with all
/// three checks in order: Allowlist → Denylist → Jurisdiction.
///
/// Returns `(engine_admin, engine_client)`.
fn setup_engine_all_three<'a>(
    env: &'a Env,
    allowlist_id: &Address,
    denylist_id: &Address,
    jurisdiction_id: &Address,
    permitted_code: &str,
) -> (Address, PolicyEngineClient<'a>) {
    let admin = Address::generate(env);
    let engine_id = env.register(PolicyEngine, ());
    let client = PolicyEngineClient::new(env, &engine_id);
    client.initialize(&admin, &CombineOp::All, &None);

    client.add_check(
        &admin,
        &CheckKind::Allowlist(AllowlistCheck {
            contract: allowlist_id.clone(),
        }),
    );
    client.add_check(
        &admin,
        &CheckKind::Denylist(DenylistCheck {
            contract: denylist_id.clone(),
        }),
    );
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction(JurisdictionCheck {
            contract: jurisdiction_id.clone(),
            allowed_codes: vec![env, String::from_str(env, permitted_code)],
        }),
    );

    (admin, client)
}

/// Onboard a single address: add to allowlist and set a permitted jurisdiction.
fn onboard(
    env: &Env,
    allowlist_id: &Address,
    allowlist_admin: &Address,
    jurisdiction_id: &Address,
    issuer: &Address,
    who: &Address,
    code: &str,
) {
    AllowlistTokenClient::new(env, allowlist_id).add_to_allowlist(allowlist_admin, who, &None);
    JurisdictionFlagClient::new(env, jurisdiction_id)
        .set_jurisdiction(issuer, who, &String::from_str(env, code));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Happy path: both `from` and `to` are allowlisted, not denylisted, and in a
/// permitted jurisdiction.  The policy-engine must return `true`.
#[test]
fn test_all_three_pass() {
    let env = Env::default();
    env.mock_all_auths();

    let allowlist_admin = Address::generate(&env);
    let denylist_admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    let allowlist_id = setup_allowlist(&env, &allowlist_admin);
    let denylist_id = setup_denylist(&env, &denylist_admin);
    let jurisdiction_id = setup_jurisdiction(&env, &issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Both parties fully onboarded.
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &from,
        PERMITTED_CODE,
    );
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &to,
        PERMITTED_CODE,
    );

    let (_admin, engine) =
        setup_engine_all_three(&env, &allowlist_id, &denylist_id, &jurisdiction_id, PERMITTED_CODE);

    assert!(
        engine.evaluate(&from, &to),
        "expected all-three-pass to return true"
    );
}

/// Allowlist failure: `from` is not on the allowlist.  All other checks would
/// pass; the engine must return `false` because the allowlist check fails.
#[test]
fn test_fail_allowlist_check() {
    let env = Env::default();
    env.mock_all_auths();

    let allowlist_admin = Address::generate(&env);
    let denylist_admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    let allowlist_id = setup_allowlist(&env, &allowlist_admin);
    let denylist_id = setup_denylist(&env, &denylist_admin);
    let jurisdiction_id = setup_jurisdiction(&env, &issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // `to` is fully onboarded.
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &to,
        PERMITTED_CODE,
    );
    // `from` has a permitted jurisdiction but is intentionally NOT added to
    // the allowlist — this is the gate that should trip the policy.
    JurisdictionFlagClient::new(&env, &jurisdiction_id)
        .set_jurisdiction(&issuer, &from, &String::from_str(&env, PERMITTED_CODE));

    let (_admin, engine) =
        setup_engine_all_three(&env, &allowlist_id, &denylist_id, &jurisdiction_id, PERMITTED_CODE);

    assert!(
        !engine.evaluate(&from, &to),
        "expected allowlist failure to return false"
    );
}

/// Denylist failure: `from` is denylisted.  Both parties are allowlisted and
/// in a permitted jurisdiction; only the denylist gate trips.
#[test]
fn test_fail_denylist_check() {
    let env = Env::default();
    env.mock_all_auths();

    let allowlist_admin = Address::generate(&env);
    let denylist_admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    let allowlist_id = setup_allowlist(&env, &allowlist_admin);
    let denylist_id = setup_denylist(&env, &denylist_admin);
    let jurisdiction_id = setup_jurisdiction(&env, &issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Both parties fully onboarded.
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &from,
        PERMITTED_CODE,
    );
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &to,
        PERMITTED_CODE,
    );

    // `from` is added to the denylist — this should cause the policy to fail.
    DenylistGateClient::new(&env, &denylist_id)
        .add_to_denylist(&denylist_admin, &from);

    let (_admin, engine) =
        setup_engine_all_three(&env, &allowlist_id, &denylist_id, &jurisdiction_id, PERMITTED_CODE);

    assert!(
        !engine.evaluate(&from, &to),
        "expected denylist failure to return false"
    );
}

/// Jurisdiction failure: `to` has a forbidden jurisdiction code (not in the
/// permitted list).  Both parties are allowlisted and neither is denylisted.
#[test]
fn test_fail_jurisdiction_check() {
    let env = Env::default();
    env.mock_all_auths();

    let allowlist_admin = Address::generate(&env);
    let denylist_admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    let allowlist_id = setup_allowlist(&env, &allowlist_admin);
    let denylist_id = setup_denylist(&env, &denylist_admin);
    let jurisdiction_id = setup_jurisdiction(&env, &issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // `from` fully onboarded with a permitted code.
    onboard(
        &env,
        &allowlist_id,
        &allowlist_admin,
        &jurisdiction_id,
        &issuer,
        &from,
        PERMITTED_CODE,
    );
    // `to` is allowlisted but is in a forbidden jurisdiction.
    AllowlistTokenClient::new(&env, &allowlist_id)
        .add_to_allowlist(&allowlist_admin, &to, &None);
    JurisdictionFlagClient::new(&env, &jurisdiction_id)
        .set_jurisdiction(&issuer, &to, &String::from_str(&env, FORBIDDEN_CODE));

    let (_admin, engine) =
        setup_engine_all_three(&env, &allowlist_id, &denylist_id, &jurisdiction_id, PERMITTED_CODE);

    assert!(
        !engine.evaluate(&from, &to),
        "expected jurisdiction failure to return false"
    );
}

/// OR semantics smoke test: the denylist check fails for both parties
/// (both are on the denylist) but the allowlist check passes for both.
/// With `CombineOp::Any` at least one check must pass for both addresses,
/// so the engine must return `true`.
#[test]
fn test_or_semantics_any_check_passes() {
    let env = Env::default();
    env.mock_all_auths();

    let allowlist_admin = Address::generate(&env);
    let denylist_admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    let allowlist_id = setup_allowlist(&env, &allowlist_admin);
    let denylist_id = setup_denylist(&env, &denylist_admin);
    // jurisdiction_id is set up but not added as a check in this test.
    let _jurisdiction_id = setup_jurisdiction(&env, &issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Both parties are allowlisted (allowlist check will pass).
    AllowlistTokenClient::new(&env, &allowlist_id)
        .add_to_allowlist(&allowlist_admin, &from, &None);
    AllowlistTokenClient::new(&env, &allowlist_id)
        .add_to_allowlist(&allowlist_admin, &to, &None);

    // Both parties are also denylisted (denylist check will fail).
    DenylistGateClient::new(&env, &denylist_id)
        .add_to_denylist(&denylist_admin, &from);
    DenylistGateClient::new(&env, &denylist_id)
        .add_to_denylist(&denylist_admin, &to);

    // Set up a policy-engine with Any semantics:
    // Allowlist → Denylist order, same two checks.
    let engine_admin = Address::generate(&env);
    let engine_id = env.register(PolicyEngine, ());
    let engine = PolicyEngineClient::new(&env, &engine_id);
    engine.initialize(&engine_admin, &CombineOp::Any, &None);

    engine.add_check(
        &engine_admin,
        &CheckKind::Allowlist(AllowlistCheck {
            contract: allowlist_id.clone(),
        }),
    );
    engine.add_check(
        &engine_admin,
        &CheckKind::Denylist(DenylistCheck {
            contract: denylist_id.clone(),
        }),
    );
    // Jurisdiction check intentionally omitted — both parties lack a code,
    // so if it were included as an Any check it would not be the passing one.
    // The allowlist check alone is the passing gate here.

    // With Any: allowlist passes for both → result should be true even though
    // denylist fails.
    assert!(
        engine.evaluate(&from, &to),
        "expected Any semantics to return true when at least one check passes for both parties"
    );
}
