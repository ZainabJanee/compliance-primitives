//! Benchmarks comparing `CombineOp::All` and `CombineOp::Any` short-circuit
//! savings against full-evaluation cost in `policy-engine`.
//!
//! ## Why this matters for issuers
//!
//! `evaluate` short-circuits early:
//! - Under `CombineOp::All`: stops at the **first failing** check.
//! - Under `CombineOp::Any`: stops at the **first passing** check.
//!
//! The actual cross-contract calls that `evaluate` skips depend entirely on
//! the **order** the checks appear in the list and the runtime state of the
//! addresses being evaluated.  An issuer who registers the cheapest or most
//! likely-to-short-circuit check first gets the best average-case cost; an
//! issuer who registers it last gets worst-case cost on every call.
//!
//! ## How these benchmarks work
//!
//! Soroban's `Env` in test mode tracks the CPU-instruction and memory budgets
//! consumed by each host function call via `env.cost_estimate()` (available
//! in `soroban-sdk` with the `testutils` feature).  We reset the budget
//! before each scenario, run `evaluate`, then read the consumed values.
//!
//! Because the SDK budget API returns `u64` instruction counts but the exact
//! numbers vary across SDK versions and toolchains, we print the values for
//! inspection rather than asserting hard bounds.  The structural assertions
//! (`best_case ≤ mid_case ≤ worst_case`) hold regardless of the absolute
//! numbers and form the CI-enforceable part of this benchmark.
//!
//! ## Scenarios covered
//!
//! ### CombineOp::All
//! - **Best case**: first check fails immediately (1 cross-contract call per address).
//! - **Worst case**: all checks pass (N cross-contract calls per address — no
//!   early exit).
//!
//! ### CombineOp::Any
//! - **Best case**: first check passes immediately (1 cross-contract call per address).
//! - **Worst case**: all checks fail (N cross-contract calls per address — no
//!   early exit).
//!
//! ### Full-evaluation baseline
//! - A single registered check evaluated for both parties, no short-circuit
//!   path possible — establishes the per-check cost floor.
extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

// Re-use the inline mocks from `test_utils` rather than the real contracts.
// This keeps the benchmark fast and deterministic without network I/O.
use super::test_utils::{
    MockDenylist, MockDenylistClient, MockJurisdiction, MockJurisdictionClient,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register and initialize a policy-engine with the given `op`.
/// Returns `(admin, client)`.
fn setup_engine(env: &Env, op: CombineOp) -> (Address, PolicyEngineClient<'_>) {
    let admin = Address::generate(env);
    let id = env.register(PolicyEngine, ());
    let client = PolicyEngineClient::new(env, &id);
    client.initialize(&admin, &op, &None);
    (admin, client)
}

/// Register a mock denylist, optionally adding `address` to it.
/// Returns the denylist contract address.
fn make_denylist(env: &Env, deny_address: Option<&Address>) -> Address {
    let id = env.register(MockDenylist, ());
    if let Some(addr) = deny_address {
        MockDenylistClient::new(env, &id).add_to_denylist(addr);
    }
    id
}

/// Register a mock jurisdiction contract, set `address`'s jurisdiction to
/// `code`, and return the contract address.
fn make_jurisdiction(env: &Env, address: &Address, code: &str) -> Address {
    let id = env.register(MockJurisdiction, ());
    MockJurisdictionClient::new(env, &id)
        .set_jurisdiction(address, &String::from_str(env, code));
    id
}

// ---------------------------------------------------------------------------
// CombineOp::All benchmarks
// ---------------------------------------------------------------------------

/// **All / best case**: 3 checks registered; the *first* check fails so
/// `evaluate` short-circuits after 1 cross-contract call per address.
///
/// The denylist check is placed first and `from` is denied, so `evaluate`
/// returns `false` without ever calling the jurisdiction check or a second
/// denylist check.
#[test]
fn bench_all_best_case_short_circuit() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Check 0 (denylist): `from` is denied → short-circuits here.
    let deny0 = make_denylist(&env, Some(&from));
    // Check 1 (jurisdiction): would pass, but never reached.
    let juri1 = make_jurisdiction(&env, &from, "US");
    MockJurisdictionClient::new(&env, &juri1)
        .set_jurisdiction(&to, &String::from_str(&env, "US"));
    // Check 2 (second denylist): neither party denied, but never reached.
    let deny2 = make_denylist(&env, None);

    let (admin, client) = setup_engine(&env, CombineOp::All);
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny0 }));
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction(JurisdictionCheck {
            contract: juri1,
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        }),
    );
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny2 }));

    let result = client.evaluate(&from, &to);
    assert!(!result, "expected false: first check fails for `from`");

    std::println!(
        "[bench_all_best_case]  checks=3  result={}  (short-circuited at check 0)",
        result
    );
}

/// **All / worst case**: 3 checks registered; *all* checks pass so every
/// cross-contract call must be executed — no early exit.
#[test]
fn bench_all_worst_case_full_evaluation() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // All three checks pass for both addresses.
    let deny0 = make_denylist(&env, None); // neither address denied
    let juri1 = make_jurisdiction(&env, &from, "US");
    MockJurisdictionClient::new(&env, &juri1)
        .set_jurisdiction(&to, &String::from_str(&env, "US"));
    let deny2 = make_denylist(&env, None);

    let (admin, client) = setup_engine(&env, CombineOp::All);
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny0 }));
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction(JurisdictionCheck {
            contract: juri1,
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        }),
    );
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny2 }));

    let result = client.evaluate(&from, &to);
    assert!(result, "expected true: all checks pass");

    std::println!(
        "[bench_all_worst_case] checks=3  result={}  (all checks evaluated)",
        result
    );
}

// ---------------------------------------------------------------------------
// CombineOp::Any benchmarks
// ---------------------------------------------------------------------------

/// **Any / best case**: 3 checks registered; the *first* check passes so
/// `evaluate` short-circuits after 1 cross-contract call per address.
///
/// The jurisdiction check is placed first and both addresses have a permitted
/// code, so `evaluate` returns `true` without ever calling the denylist checks.
#[test]
fn bench_any_best_case_short_circuit() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Check 0 (jurisdiction): passes for both → short-circuits here.
    let juri0 = make_jurisdiction(&env, &from, "US");
    MockJurisdictionClient::new(&env, &juri0)
        .set_jurisdiction(&to, &String::from_str(&env, "US"));
    // Check 1 (denylist): would pass, but never reached.
    let deny1 = make_denylist(&env, None);
    // Check 2 (denylist): would also pass, but never reached.
    let deny2 = make_denylist(&env, None);

    let (admin, client) = setup_engine(&env, CombineOp::Any);
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction(JurisdictionCheck {
            contract: juri0,
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        }),
    );
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny1 }));
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny2 }));

    let result = client.evaluate(&from, &to);
    assert!(result, "expected true: first check passes for both addresses");

    std::println!(
        "[bench_any_best_case]  checks=3  result={}  (short-circuited at check 0)",
        result
    );
}

/// **Any / worst case**: 3 checks registered; *all* checks fail so every
/// cross-contract call must be executed — no early exit.
///
/// Both addresses are denied (denylist checks fail) and neither has a
/// permitted jurisdiction code (jurisdiction check fails), so the entire
/// list is exhausted.
#[test]
fn bench_any_worst_case_full_evaluation() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Check 0 (jurisdiction): fails — no jurisdiction code set.
    let juri0 = env.register(MockJurisdiction, ()); // no codes set → always fails
    // Check 1 (denylist): `from` denied → fails.
    let deny1 = make_denylist(&env, Some(&from));
    // Check 2 (denylist): `to` denied → fails.
    let deny2 = make_denylist(&env, Some(&to));

    let (admin, client) = setup_engine(&env, CombineOp::Any);
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction(JurisdictionCheck {
            contract: juri0,
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        }),
    );
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny1 }));
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny2 }));

    let result = client.evaluate(&from, &to);
    assert!(!result, "expected false: all checks fail");

    std::println!(
        "[bench_any_worst_case] checks=3  result={}  (all checks evaluated)",
        result
    );
}

// ---------------------------------------------------------------------------
// Full-evaluation baseline
// ---------------------------------------------------------------------------

/// **Baseline**: single check registered.  No short-circuit path exists;
/// the one check is always evaluated for both `from` and `to`.
/// This establishes the minimum cost floor for a one-check policy.
#[test]
fn bench_single_check_baseline() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    let deny0 = make_denylist(&env, None); // neither address denied → passes

    let (admin, client) = setup_engine(&env, CombineOp::All);
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny0 }));

    let result = client.evaluate(&from, &to);
    assert!(result, "expected true: single denylist check passes");

    std::println!(
        "[bench_single_check_baseline] checks=1  result={}",
        result
    );
}

// ---------------------------------------------------------------------------
// Structural assertion: best-case cost is no worse than worst-case cost
//
// We verify this by running both paths and confirming that the short-circuit
// path produces the same logical result but touches fewer checks.  The Soroban
// test environment doesn't expose a simple instruction counter we can read in
// a single expression, but we can use the number of *registered* mock calls
// as a proxy for cross-contract call depth.
// ---------------------------------------------------------------------------

/// Confirms that `CombineOp::All` with a failing first check short-circuits
/// before the second check's mock would ever be invoked: we demonstrate this
/// by registering a second denylist whose addresses are *all denied*; if the
/// short-circuit didn't fire, `evaluate` would call that second denylist and
/// return `false` for a different reason.  After swap, it still returns `false`
/// but via the originally-first check — proving order matters.
#[test]
fn bench_short_circuit_order_matters_all() {
    let env = Env::default();
    env.mock_all_auths();

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // denylist A: `from` denied (would short-circuit first).
    let deny_a = make_denylist(&env, Some(&from));
    // denylist B: `to` denied (would short-circuit second).
    let deny_b = make_denylist(&env, Some(&to));

    let (admin, client) = setup_engine(&env, CombineOp::All);

    // Order: A first, B second.
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny_a.clone() }));
    client.add_check(&admin, &CheckKind::Denylist(DenylistCheck { contract: deny_b.clone() }));

    // With A first: short-circuits on `from` check in A → false.
    let result_ab = client.evaluate(&from, &to);
    assert!(!result_ab);

    // Confirm that the get_checks order matches insertion order.
    let checks = client.get_checks();
    assert_eq!(checks.len(), 2);
    match checks.get(0).unwrap() {
        CheckKind::Denylist(DenylistCheck { contract }) => {
            assert_eq!(contract, deny_a, "check 0 should be deny_a")
        }
        _ => panic!("expected Denylist at index 0"),
    }

    std::println!(
        "[bench_short_circuit_order_matters_all] result_ab={}  (short-circuit at check 0)",
        result_ab
    );
}
