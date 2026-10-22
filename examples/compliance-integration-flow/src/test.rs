//! Integration test (#218) composing seven contracts in one transfer flow:
//! `jurisdiction-flag`, `policy-engine`, `compliance-aggregator`,
//! `multisig-admin`, `circuit-breaker`, and `audit-log` (plus `pausable`,
//! used internally by `jurisdiction-flag`).
//!
//! See the crate-level doc comment in `lib.rs` for why `allowlist-token` and
//! `denylist-gate` are excluded (pre-existing, unrelated corruption in both
//! contracts).

extern crate std;

use super::*;
use audit_log::{AuditLog, AuditLogClient as AuditLogTestClient};
use circuit_breaker::{CircuitBreaker, CircuitBreakerClient as CircuitBreakerTestClient};
use compliance_aggregator::{ComplianceAggregator, ComplianceAggregatorClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use multisig_admin::{MultisigAdmin, MultisigAdminClient};
use policy_engine::{CheckKind, CombineOp, PolicyEngine, PolicyEngineClient as PolicyEngineTestClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

/// Deploys and wires up all seven contracts, returning the addresses needed
/// to call `execute_transfer`, plus the `jurisdiction-flag` issuer and
/// the `audit-log` source identity, needed by the scenario tests below.
#[allow(clippy::type_complexity)]
fn setup_with_issuer(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    env.mock_all_auths();

    let issuer = Address::generate(env);
    let jflag_id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &jflag_id).initialize(&issuer);

    let policy_admin = Address::generate(env);
    let policy_id = env.register(PolicyEngine, ());
    let policy_client = PolicyEngineTestClient::new(env, &policy_id);
    policy_client.initialize(&policy_admin, &CombineOp::All);
    let us_codes = vec![env, String::from_str(env, "US")];
    policy_client.add_check(
        &policy_admin,
        &CheckKind::Jurisdiction {
            contract: jflag_id.clone(),
            allowed_codes: us_codes,
        },
    );

    let agg_admin = Address::generate(env);
    let agg_id = env.register(ComplianceAggregator, ());
    ComplianceAggregatorClient::new(env, &agg_id).initialize(
        &agg_admin,
        &None,
        &Some(jflag_id.clone()),
    );

    let signer_a = Address::generate(env);
    let signer_b = Address::generate(env);
    let multisig_id = env.register(MultisigAdmin, ());
    MultisigAdminClient::new(env, &multisig_id)
        .initialize(&vec![env, signer_a.clone(), signer_b.clone()], &2u32);

    let breaker_id = env.register(CircuitBreaker, ());
    CircuitBreakerTestClient::new(env, &breaker_id).initialize(&multisig_id);

    let audit_admin = Address::generate(env);
    let audit_id = env.register(AuditLog, ());
    AuditLogTestClient::new(env, &audit_id).initialize(&audit_admin);

    let audit_source = Address::generate(env);

    (
        issuer, jflag_id, policy_id, agg_id, multisig_id, breaker_id, audit_id, audit_source,
    )
}

#[test]
fn test_allowed_transfer_is_permitted_and_logged() {
    let env = Env::default();
    let (issuer, jflag_id, policy_id, agg_id, _multisig_id, breaker_id, audit_id, audit_source) =
        setup_with_issuer(&env);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    JurisdictionFlagClient::new(&env, &jflag_id).set_jurisdiction(
        &issuer,
        &from,
        &String::from_str(&env, "US"),
    );
    JurisdictionFlagClient::new(&env, &jflag_id).set_jurisdiction(
        &issuer,
        &to,
        &String::from_str(&env, "US"),
    );

    let result = execute_transfer(
        &env,
        &policy_id,
        &breaker_id,
        &audit_id,
        &audit_source,
        &from,
        &to,
    );
    assert_eq!(result, Ok(true));

    // Cross-check against the aggregator's independent view.
    let (agg_passed, _) = ComplianceAggregatorClient::new(&env, &agg_id)
        .check_address(&to, &vec![&env, String::from_str(&env, "US")]);
    assert!(agg_passed);

    // The allowed decision was recorded to the audit log.
    let entry = AuditLogTestClient::new(&env, &audit_id).get_entry(&0);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().subject, to);
}

#[test]
fn test_blocked_transfer_wrong_jurisdiction_is_logged() {
    let env = Env::default();
    let (issuer, jflag_id, policy_id, _agg_id, _multisig_id, breaker_id, audit_id, audit_source) =
        setup_with_issuer(&env);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // `from` is permitted, `to` is not (no jurisdiction set at all).
    JurisdictionFlagClient::new(&env, &jflag_id).set_jurisdiction(
        &issuer,
        &from,
        &String::from_str(&env, "US"),
    );

    let result = execute_transfer(
        &env,
        &policy_id,
        &breaker_id,
        &audit_id,
        &audit_source,
        &from,
        &to,
    );
    assert_eq!(result, Ok(false));

    let entry = AuditLogTestClient::new(&env, &audit_id)
        .get_entry(&0)
        .unwrap();
    assert_eq!(entry.kind, soroban_sdk::symbol_short!("blocked"));
}

#[test]
fn test_circuit_breaker_halts_flow_before_policy_check() {
    let env = Env::default();
    let (issuer, jflag_id, policy_id, _agg_id, multisig_id, breaker_id, audit_id, audit_source) =
        setup_with_issuer(&env);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Even a fully-compliant pair of addresses must be halted once frozen.
    JurisdictionFlagClient::new(&env, &jflag_id).set_jurisdiction(
        &issuer,
        &from,
        &String::from_str(&env, "US"),
    );
    JurisdictionFlagClient::new(&env, &jflag_id).set_jurisdiction(
        &issuer,
        &to,
        &String::from_str(&env, "US"),
    );

    // The multisig-admin contract is the circuit-breaker's admin; under
    // `mock_all_auths` its `require_auth` (routed through `__check_auth`)
    // is satisfied for this call.
    CircuitBreakerTestClient::new(&env, &breaker_id).freeze(&multisig_id, &None);
    assert!(CircuitBreakerTestClient::new(&env, &breaker_id).is_frozen());

    let result = execute_transfer(
        &env,
        &policy_id,
        &breaker_id,
        &audit_id,
        &audit_source,
        &from,
        &to,
    );
    assert_eq!(result, Err(FlowError::CircuitBreakerFrozen));

    let entry = AuditLogTestClient::new(&env, &audit_id)
        .get_entry(&0)
        .unwrap();
    assert_eq!(entry.kind, soroban_sdk::symbol_short!("halted"));

    // Unfreezing restores normal flow.
    CircuitBreakerTestClient::new(&env, &breaker_id).unfreeze(&multisig_id);
    let result = execute_transfer(
        &env,
        &policy_id,
        &breaker_id,
        &audit_id,
        &audit_source,
        &from,
        &to,
    );
    assert_eq!(result, Ok(true));
}
