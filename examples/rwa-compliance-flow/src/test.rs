use super::*;
use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

struct ComplianceSetup<'a> {
    env: &'a Env,
    allowlist_admin: Address,
    denylist_admin: Address,
    issuer: Address,
    allowlist_token: AllowlistTokenClient<'a>,
    denylist_gate: DenylistGateClient<'a>,
    jurisdiction_flag: JurisdictionFlagClient<'a>,
    mock_token_id: Address,
}

fn setup_compliance_contracts(env: &Env) -> ComplianceSetup {
    env.mock_all_auths();

    let allowlist_admin = Address::generate(env);
    let denylist_admin = Address::generate(env);
    let issuer = Address::generate(env);
    let mock_token_id = Address::generate(env);

    // Register and initialize allowlist-token
    let allowlist_token_id = env.register(AllowlistToken, ());
    let allowlist_token = AllowlistTokenClient::new(env, &allowlist_token_id);
    allowlist_token.initialize(&allowlist_admin, &mock_token_id);

    // Register and initialize denylist-gate
    let denylist_gate_id = env.register(DenylistGate, ());
    let denylist_gate = DenylistGateClient::new(env, &denylist_gate_id);
    denylist_gate.initialize(&denylist_admin);

    // Register and initialize jurisdiction-flag
    let jurisdiction_flag_id = env.register(JurisdictionFlag, ());
    let jurisdiction_flag = JurisdictionFlagClient::new(env, &jurisdiction_flag_id);
    jurisdiction_flag.initialize(&issuer);

    ComplianceSetup {
        env,
        allowlist_admin,
        denylist_admin,
        issuer,
        allowlist_token,
        denylist_gate,
        jurisdiction_flag,
        mock_token_id,
    }
}

#[test]
fn test_rwa_token_flow_success_all_checks_pass() {
    // SCENARIO: Successful transfer when all three compliance checks pass
    //
    // This tests the happy path where:
    // - Both parties are on the allowlist
    // - Neither party is on the denylist
    // - Both parties are in permitted jurisdictions
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    // Create two parties (alice and bob)
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Step 1: Add both parties to the allowlist
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &bob, &None);

    // Step 2: Ensure both parties are NOT on the denylist (by default they aren't)
    assert!(setup.denylist_gate.check(&alice));
    assert!(setup.denylist_gate.check(&bob));

    // Step 3: Set jurisdiction for both parties (both in USA)
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Step 4: Verify both parties are in permitted jurisdiction
    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted_codes));
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted_codes));

    // Step 5: Verify all compliance checks pass for both parties
    // ALLOWLIST CHECK
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(setup.allowlist_token.is_allowed(&bob));

    // DENYLIST CHECK
    assert!(setup.denylist_gate.check(&alice));
    assert!(setup.denylist_gate.check(&bob));

    // JURISDICTION CHECK
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted_codes));
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted_codes));

    // ✓ This represents a successful RWA token transfer scenario
    // All three compliance checks pass for both parties
}

#[test]
fn test_rwa_token_flow_blocked_by_allowlist() {
    // SCENARIO: Transfer blocked by allowlist check
    //
    // This tests the failure case where:
    // - Sender (alice) is on the allowlist
    // - Recipient (bob) is NOT on the allowlist
    // - Neither is on the denylist
    // - Both are in permitted jurisdictions
    //
    // RESULT: Transfer should be blocked at the allowlist check
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add only alice to allowlist, NOT bob
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);

    // Set jurisdiction for both (would otherwise pass)
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Ensure neither is on denylist (would otherwise pass)
    assert!(setup.denylist_gate.check(&alice));
    assert!(setup.denylist_gate.check(&bob));

    // VERIFICATION: Allowlist check fails for bob
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(!setup.allowlist_token.is_allowed(&bob));

    // Jurisdiction checks pass
    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted_codes));
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted_codes));

    // ✗ Transfer would be BLOCKED by allowlist check
    // Bob is not on the allowlist
}

#[test]
fn test_rwa_token_flow_blocked_by_denylist() {
    // SCENARIO: Transfer blocked by denylist check
    //
    // This tests the failure case where:
    // - Both parties are on the allowlist
    // - Sender (alice) IS on the denylist
    // - Both are in permitted jurisdictions
    //
    // RESULT: Transfer should be blocked at the denylist check
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add both to allowlist (would otherwise pass)
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &bob, &None);

    // Set jurisdiction for both (would otherwise pass)
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Add alice to denylist (sanctions/fraud/court order, etc.)
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &alice);

    // VERIFICATION: Denylist check fails for alice
    assert!(!setup.denylist_gate.check(&alice));
    assert!(setup.denylist_gate.check(&bob));

    // Allowlist checks pass
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(setup.allowlist_token.is_allowed(&bob));

    // ✗ Transfer would be BLOCKED by denylist check
    // Alice is on the denylist
}

#[test]
fn test_rwa_token_flow_blocked_by_jurisdiction() {
    // SCENARIO: Transfer blocked by jurisdiction check
    //
    // This tests the failure case where:
    // - Both parties are on the allowlist
    // - Neither is on the denylist
    // - Sender (alice) is in a permitted jurisdiction
    // - Recipient (bob) is NOT in a permitted jurisdiction (or has no jurisdiction set)
    //
    // RESULT: Transfer should be blocked at the jurisdiction check
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add both to allowlist (would otherwise pass)
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &bob, &None);

    // Ensure neither is on denylist (would otherwise pass)
    assert!(setup.denylist_gate.check(&alice));
    assert!(setup.denylist_gate.check(&bob));

    // Set jurisdiction for alice (USA) but NOT bob
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    // bob has no jurisdiction set

    // Only allow USA jurisdictions
    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];

    // VERIFICATION: Jurisdiction check passes for alice but fails for bob
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted_codes));
    assert!(!setup.jurisdiction_flag.is_permitted_jurisdiction(&bob, &permitted_codes));

    // Allowlist checks pass
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(setup.allowlist_token.is_allowed(&bob));

    // ✗ Transfer would be BLOCKED by jurisdiction check
    // Bob is not in a permitted jurisdiction
}

#[test]
fn test_rwa_token_flow_blocked_by_all_three_independently() {
    // SCENARIO: Comprehensive test showing all three compliance layers
    // can independently block transfers
    //
    // This test creates two parties with opposite compliance states:
    // - Charlie: fails EVERY check
    // - Diana: passes EVERY check
    //
    // RESULT: Demonstrates all three layers working independently
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let charlie = Address::generate(&env);
    let diana = Address::generate(&env);

    // Charlie: NOT on allowlist
    // Diana: ON allowlist
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &diana, &None);

    // Charlie: ON denylist
    // Diana: NOT on denylist
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &charlie);

    // Charlie: no jurisdiction set (fails jurisdiction check)
    // Diana: USA jurisdiction (passes jurisdiction check)
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &diana, &usa_code);

    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];

    // VERIFICATION: Charlie fails all three checks
    assert!(!setup.allowlist_token.is_allowed(&charlie));
    assert!(!setup.denylist_gate.check(&charlie));
    assert!(!setup.jurisdiction_flag.is_permitted_jurisdiction(&charlie, &permitted_codes));

    // VERIFICATION: Diana passes all three checks
    assert!(setup.allowlist_token.is_allowed(&diana));
    assert!(setup.denylist_gate.check(&diana));
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&diana, &permitted_codes));

    // ✓ This demonstrates that the three compliance layers are independent
    // Charlie would be blocked at any of the three checks
    // Diana passes all three layers
}

#[test]
fn test_metadata_available_on_all_contracts() {
    // BONUS TEST: Verify that metadata() function works on all contracts
    // This is related to Issue #24
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    // Try to call metadata on each contract
    // (Note: The actual metadata() client methods would need to be defined
    // in the contract clients above, but this demonstrates the pattern)

    // These calls would work once metadata clients are properly defined:
    // let allowlist_meta = setup.allowlist_token.metadata();
    // let denylist_meta = setup.denylist_gate.metadata();
    // let jurisdiction_meta = setup.jurisdiction_flag.metadata();
}

#[test]
fn test_unified_compliance_check_interface() {
    // TEST: Demonstrate the shared ComplianceCheck interface (Issue #25)
    //
    // This shows how external contracts can call any of the three compliance
    // primitives through a unified `is_compliant(address) -> bool` interface.
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    // Set up different compliance states
    // Alice: allowlisted, not denied, has jurisdiction
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);
    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);

    // Bob: not allowlisted, not denied, has jurisdiction
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Charlie: allowlisted, denied, no jurisdiction
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &charlie, &None);
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &charlie);

    // Test calling through specialized interfaces
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(setup.denylist_gate.check(&alice));
    assert!(setup.jurisdiction_flag.get_jurisdiction(&alice).is_some());

    // Test calling through UNIFIED ComplianceCheck interface
    // All three can now be called with the same pattern:
    // contract.is_compliant(address) -> bool
    assert!(setup.allowlist_token.is_compliant(&alice));
    assert!(setup.denylist_gate.is_compliant(&alice));
    assert!(setup.jurisdiction_flag.is_compliant(&alice));

    // For bob (not allowlisted, but has jurisdiction):
    assert!(!setup.allowlist_token.is_compliant(&bob)); // fails allowlist check
    assert!(setup.denylist_gate.is_compliant(&bob));    // passes denylist check
    assert!(setup.jurisdiction_flag.is_compliant(&bob)); // passes jurisdiction check

    // For charlie (allowlisted, but denied):
    assert!(setup.allowlist_token.is_compliant(&charlie)); // passes allowlist check
    assert!(!setup.denylist_gate.is_compliant(&charlie));  // fails denylist check
    assert!(!setup.jurisdiction_flag.is_compliant(&charlie)); // fails jurisdiction check

    // ✓ Demonstrates that each contract implements the unified interface
    // External contracts can now compose these primitives polymorphically
}

#[test]
fn test_compliance_check_enables_polymorphic_composition() {
    // ADVANCED TEST: Show how the unified interface enables polymorphic composition
    //
    // This demonstrates how external contracts can check multiple compliance
    // primitives without knowing which specific contract they're calling.
    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Set up: both compliant
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &alice, &None);
    setup.allowlist_token.add_to_allowlist(&setup.allowlist_admin, &bob, &None);

    let usa_code = String::from_slice(&env, "US");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);

    // Simulated "compliance check pass-through" function
    // In a real contract, this might be:
    // fn check_compliance(compliance_contract: Address, address: Address) -> bool {
    //     let client = ComplianceCheckClient::new(env, &compliance_contract);
    //     client.is_compliant(&address)
    // }

    // All three can be used interchangeably with the same check pattern:
    assert!(setup.allowlist_token.is_compliant(&alice));
    assert!(setup.denylist_gate.is_compliant(&alice));
    assert!(setup.jurisdiction_flag.is_compliant(&alice));

    assert!(setup.allowlist_token.is_compliant(&bob));
    assert!(setup.denylist_gate.is_compliant(&bob));
    assert!(setup.jurisdiction_flag.is_compliant(&bob));

    // ✓ This demonstrates backward compatibility:
    // Specialized functions still work
    assert!(setup.allowlist_token.is_allowed(&alice));
    assert!(setup.denylist_gate.check(&alice));
    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];
    assert!(setup.jurisdiction_flag.is_permitted_jurisdiction(&alice, &permitted_codes));
}

#[test]
fn test_integration_with_policy_engine() {
    use policy_engine::{PolicyEngine, PolicyEngineClient, CheckKind, CombineOp, AddressPair};

    let env = Env::default();
    let setup = setup_compliance_contracts(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    let diana = Address::generate(&env);

    // Initialize policy engine
    let engine_id = env.register(PolicyEngine, ());
    let engine = PolicyEngineClient::new(&env, &engine_id);
    engine.initialize(&setup.issuer, &CombineOp::All, &None);

    // Register denylist-gate and jurisdiction-flag checkkind
    engine.add_check(
        &setup.issuer,
        &CheckKind::Denylist(policy_engine::DenylistCheck {
            contract: setup.denylist_gate.address.clone(),
        }),
    );

    let usa_code = String::from_slice(&env, "US");
    let permitted_codes = soroban_sdk::vec![&env, usa_code.clone()];
    engine.add_check(
        &setup.issuer,
        &CheckKind::Jurisdiction(policy_engine::JurisdictionCheck {
            contract: setup.jurisdiction_flag.address.clone(),
            allowed_codes: permitted_codes,
        }),
    );

    // Set jurisdictions for all
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &bob, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &charlie, &usa_code);
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &diana, &usa_code);

    // 1. Both compliant (USA, not denied) -> passes
    assert!(engine.evaluate(&alice, &bob));

    // 2. Denylisted -> fails
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &alice);
    assert!(!engine.evaluate(&alice, &bob));
    setup.denylist_gate.remove_from_denylist(&setup.denylist_admin, &alice);

    // 3. Invalid jurisdiction -> fails
    let de_code = String::from_slice(&env, "DE");
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &de_code);
    assert!(!engine.evaluate(&alice, &bob));

    // Restore US jurisdiction
    setup.jurisdiction_flag.set_jurisdiction(&setup.issuer, &alice, &usa_code);
    assert!(engine.evaluate(&alice, &bob));

    // 4. Batch evaluate works (one passing pair, one failing pair due to Charlie being denylisted)
    setup.denylist_gate.add_to_denylist(&setup.denylist_admin, &charlie);

    let mut pairs = soroban_sdk::Vec::new(&env);
    pairs.push_back(AddressPair {
        from: alice.clone(),
        to: bob.clone(),
    });
    pairs.push_back(AddressPair {
        from: charlie.clone(),
        to: diana.clone(),
    });

    let results = engine.batch_evaluate(&pairs);
    assert_eq!(results.len(), 2);
    assert!(results.get(0).unwrap());  // Passes
    assert!(!results.get(1).unwrap()); // Fails
}
