use super::*;
use soroban_sdk::testutils::{storage::Persistent as _, Address as _, Events as _, Ledger as _};
use soroban_sdk::{vec, Env};
use std::path::{Path, PathBuf};

fn setup(env: &Env) -> (Address, Address, JurisdictionFlagClient<'_>) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(env, &contract_id);
    client.initialize(&issuer);
    (issuer, contract_id, client)
}

fn read_baseline(path: &Path, section: &str) -> (u64, u64) {
    let contents = std::fs::read_to_string(path).unwrap();
    let section_header = format!("[{section}]");
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

fn baseline_path_for_manifest_dir(manifest_dir: PathBuf) -> PathBuf {
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

#[test]
fn test_set_and_get_jurisdiction() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    assert_eq!(client.get_jurisdiction(&alice), None);

    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);
    assert_eq!(client.get_jurisdiction(&alice), Some(code));
}

#[test]
fn test_budget_regression_is_permitted_jurisdiction() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed = vec![
        &env,
        String::from_str(&env, "CA"),
        String::from_str(&env, "US"),
    ];

    let mut budget = env.cost_estimate().budget();
    budget.reset_default();
    let is_permitted = client.is_permitted_jurisdiction(&alice, &allowed);
    assert!(is_permitted);

    let measured = (budget.cpu_instruction_cost(), budget.memory_bytes_cost());
    let baseline_path = baseline_path_for_manifest_dir(PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()));
    let baseline = read_baseline(&baseline_path, "jurisdiction-flag.is_permitted_jurisdiction");
    assert_budget_within_threshold(measured, baseline, "jurisdiction-flag jurisdiction check");
}

#[test]
fn test_set_jurisdiction_rejects_non_issuer() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    let result = client.try_set_jurisdiction(&impostor, &alice, &code);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

#[test]
fn test_remove_jurisdiction_multiple_clears_mixed_addresses() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let never_set = Address::generate(&env);

    client.set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    client.set_jurisdiction(&issuer, &bob, &String::from_str(&env, "CA"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
    assert_eq!(
        client.get_jurisdiction(&bob),
        Some(String::from_str(&env, "CA"))
    );
    assert_eq!(client.get_jurisdiction(&never_set), None);

    let addresses = vec![&env, alice.clone(), bob.clone(), never_set.clone()];
    client.remove_jurisdiction_multiple(&issuer, &addresses);

    assert_eq!(client.get_jurisdiction(&alice), None);
    assert_eq!(client.get_jurisdiction(&bob), None);
    assert_eq!(client.get_jurisdiction(&never_set), None);
}

#[test]
fn test_remove_jurisdiction_multiple_rejects_non_issuer() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    client.set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));

    let addresses = vec![&env, alice.clone()];
    let result = client.try_remove_jurisdiction_multiple(&impostor, &addresses);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_remove_jurisdiction_multiple_empty_vec_is_noop() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    client.set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));

    let empty: Vec<Address> = vec![&env];
    client.remove_jurisdiction_multiple(&issuer, &empty);

    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_is_permitted_jurisdiction_true_when_code_in_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed = vec![
        &env,
        String::from_str(&env, "CA"),
        String::from_str(&env, "US"),
    ];
    assert_eq!(client.is_permitted_jurisdiction(&alice, &allowed), true);
}

#[test]
fn test_is_permitted_jurisdiction_false_when_no_jurisdiction_set() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let allowed = vec![&env, String::from_str(&env, "US")];
    assert_eq!(client.is_permitted_jurisdiction(&alice, &allowed), false);
}

#[test]
fn test_is_permitted_jurisdiction_errors_with_empty_allowed_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed: Vec<String> = vec![&env];
    let result = client.try_is_permitted_jurisdiction(&alice, &allowed);
    assert_eq!(result, Err(Ok(Error::EmptyAllowedCodes)));
}

#[test]
fn test_is_permitted_jurisdiction_errors_when_no_jurisdiction_and_empty_allowed_list() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    let allowed: Vec<String> = vec![&env];
    let result = client.try_is_permitted_jurisdiction(&alice, &allowed);
    assert_eq!(result, Err(Ok(Error::EmptyAllowedCodes)));
}

#[test]
fn test_set_jurisdiction_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    let result = client.try_set_jurisdiction(&issuer, &alice, &code);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
    assert_eq!(env.events().all(), vec![&env]);
}

#[test]
fn test_set_jurisdiction_emits_jurisdiction_set_event() {
    let env = Env::default();
    let (issuer, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    client.set_jurisdiction(&issuer, &alice, &code);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "jurisdiction_set"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::from_array(
                    &env,
                    [(Symbol::new(&env, "code"), code.clone().into_val(&env))]
                )
                .into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&issuer);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_set_jurisdiction_extends_persistent_ttl() {
    let env = Env::default();
    let (issuer, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    client.set_jurisdiction(&issuer, &alice, &code);

    let key = DataKey::Jurisdiction(alice.clone());

    // Advance the ledger until the entry TTL drops below the extension threshold.
    env.ledger().with_mut(|li| {
        li.sequence_number += super::TTL_EXTEND_TO - super::TTL_THRESHOLD + 1;
    });

    let ttl_before_read = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert!(ttl_before_read < super::TTL_THRESHOLD);

    assert_eq!(client.get_jurisdiction(&alice), Some(code));

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert_eq!(ttl_after_read, super::TTL_EXTEND_TO);

    env.ledger().with_mut(|li| {
        li.sequence_number += super::TTL_EXTEND_TO - super::TTL_THRESHOLD + 1;
    });

    let ttl_before_rewrite = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert!(ttl_before_rewrite < super::TTL_THRESHOLD);

    let updated = String::from_str(&env, "CA");
    client.set_jurisdiction(&issuer, &alice, &updated);

    let ttl_after_write = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert_eq!(ttl_after_write, super::TTL_EXTEND_TO);
}

// ---------------------------------------------------------------------------
// Issue #604: set_multiple_jurisdictions batch setter
// ---------------------------------------------------------------------------

#[test]
fn test_set_multiple_jurisdictions_basic() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "US")),
        (bob.clone(), String::from_str(&env, "CA")),
    ];
    client.set_multiple_jurisdictions(&issuer, &entries);

    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
    assert_eq!(
        client.get_jurisdiction(&bob),
        Some(String::from_str(&env, "CA"))
    );
}

#[test]
fn test_set_multiple_jurisdictions_rejects_unauthorized_caller() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "US")),
    ];
    let result = client.try_set_multiple_jurisdictions(&impostor, &entries);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

#[test]
fn test_set_multiple_jurisdictions_one_invalid_code_rejects_whole_batch() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // "us" is lowercase — invalid; the whole batch must be rejected.
    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "US")),
        (bob.clone(), String::from_str(&env, "us")),
    ];
    let result = client.try_set_multiple_jurisdictions(&issuer, &entries);
    assert_eq!(result, Err(Ok(Error::InvalidInput)));

    // Nothing should have been written.
    assert_eq!(client.get_jurisdiction(&alice), None);
    assert_eq!(client.get_jurisdiction(&bob), None);
}

#[test]
fn test_set_multiple_jurisdictions_empty_batch_is_invalid() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);

    let entries: Vec<(Address, String)> = vec![&env];
    let result = client.try_set_multiple_jurisdictions(&issuer, &entries);
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_set_multiple_jurisdictions_over_cap_is_invalid() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);

    // Build MAX_BATCH_SIZE + 1 entries.
    let mut entries_vec = soroban_sdk::vec![&env];
    for _ in 0..=super::MAX_BATCH_SIZE {
        entries_vec.push_back((Address::generate(&env), String::from_str(&env, "US")));
    }
    let result = client.try_set_multiple_jurisdictions(&issuer, &entries_vec);
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
}

#[test]
fn test_set_multiple_jurisdictions_compliance_officer_can_set() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    let alice = Address::generate(&env);

    client.set_compliance_officer(&issuer, &officer);

    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "US")),
    ];
    client.set_multiple_jurisdictions(&officer, &entries);

    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_set_multiple_jurisdictions_paused_contract_is_rejected() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.pause(&issuer);

    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "US")),
    ];
    let result = client.try_set_multiple_jurisdictions(&issuer, &entries);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

#[test]
fn test_set_multiple_jurisdictions_empty_code_is_invalid() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    let entries = vec![
        &env,
        (alice.clone(), String::from_str(&env, "")),
    ];
    let result = client.try_set_multiple_jurisdictions(&issuer, &entries);
    assert_eq!(result, Err(Ok(Error::InvalidInput)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

// ---------------------------------------------------------------------------
// Issue #603: two-step issuer transfer (propose / accept / cancel)
// ---------------------------------------------------------------------------

#[test]
fn test_propose_then_accept_issuer_transfers_control() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let new_issuer = Address::generate(&env);
    let alice = Address::generate(&env);

    client.propose_issuer(&issuer, &new_issuer);
    client.accept_issuer(&new_issuer);

    // Old issuer no longer has control.
    let result = client.try_set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // New issuer does.
    client.set_jurisdiction(&new_issuer, &alice, &String::from_str(&env, "US"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_propose_issuer_emits_event() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let new_issuer = Address::generate(&env);

    client.propose_issuer(&issuer, &new_issuer);

    let events = env.events().all();
    // There should be at least one event (IssuerProposed).
    assert!(!events.is_empty());
}

#[test]
fn test_accept_issuer_emits_event() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let new_issuer = Address::generate(&env);

    client.propose_issuer(&issuer, &new_issuer);
    let events_before = env.events().all().len();

    client.accept_issuer(&new_issuer);

    let events_after = env.events().all().len();
    assert!(events_after > events_before, "accept_issuer must emit an event");
}

#[test]
fn test_accept_issuer_with_no_pending_proposal_errors() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let random = Address::generate(&env);

    let result = client.try_accept_issuer(&random);
    assert_eq!(result, Err(Ok(Error::NoPendingIssuer)));
}

#[test]
fn test_accept_issuer_wrong_address_errors() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let proposed = Address::generate(&env);
    let wrong = Address::generate(&env);

    client.propose_issuer(&issuer, &proposed);

    let result = client.try_accept_issuer(&wrong);
    assert_eq!(result, Err(Ok(Error::PendingIssuerMismatch)));

    // Original issuer still in control.
    let alice = Address::generate(&env);
    client.set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_cancel_issuer_proposal_prevents_accept() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let new_issuer = Address::generate(&env);

    client.propose_issuer(&issuer, &new_issuer);
    client.cancel_issuer_proposal(&issuer);

    // The proposal was cancelled; accept must now fail.
    let result = client.try_accept_issuer(&new_issuer);
    assert_eq!(result, Err(Ok(Error::NoPendingIssuer)));

    // Original issuer is still in control.
    let alice = Address::generate(&env);
    client.set_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_cancel_issuer_proposal_emits_event() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let new_issuer = Address::generate(&env);

    client.propose_issuer(&issuer, &new_issuer);
    let before = env.events().all().len();

    client.cancel_issuer_proposal(&issuer);

    assert!(
        env.events().all().len() > before,
        "cancel_issuer_proposal must emit an event"
    );
}

#[test]
fn test_propose_issuer_requires_current_issuer_auth() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_propose_issuer(&impostor, &new_issuer);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_transfer_issuer_emergency_rotation_clears_pending_proposal() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let pending = Address::generate(&env);
    let emergency_new = Address::generate(&env);

    // Set up a pending proposal, then immediately do an emergency rotation.
    client.propose_issuer(&issuer, &pending);
    client.transfer_issuer(&issuer, &emergency_new);

    // The pending address must no longer be able to accept.
    let result = client.try_accept_issuer(&pending);
    assert_eq!(result, Err(Ok(Error::NoPendingIssuer)));

    // emergency_new is now issuer.
    let alice = Address::generate(&env);
    client.set_jurisdiction(&emergency_new, &alice, &String::from_str(&env, "US"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "US"))
    );
}

#[test]
fn test_propose_issuer_overwrites_previous_proposal() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.propose_issuer(&issuer, &first);
    // Overwrite with a new proposal.
    client.propose_issuer(&issuer, &second);

    // First proposed address can no longer accept.
    let result = client.try_accept_issuer(&first);
    assert_eq!(result, Err(Ok(Error::PendingIssuerMismatch)));

    // Second proposed address can accept.
    client.accept_issuer(&second);

    let alice = Address::generate(&env);
    client.set_jurisdiction(&second, &alice, &String::from_str(&env, "DE"));
    assert_eq!(
        client.get_jurisdiction(&alice),
        Some(String::from_str(&env, "DE"))
    );
}
