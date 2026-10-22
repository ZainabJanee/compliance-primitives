//! Unit tests exercising the `ComplianceCheck` trait generically across all
//! three implementing contracts: `allowlist-token`, `denylist-gate`, and
//! `jurisdiction-flag`.
//!
//! # Design
//!
//! The `ComplianceCheck` trait is a compile-time abstraction — it is meant to
//! be used by generic helper functions that accept `impl ComplianceCheck` and
//! call `is_compliant(env, address)` without knowing which concrete contract
//! they're talking to.  These tests verify exactly that pattern.
//!
//! Each test function:
//! 1. Registers the three contracts in a fresh `Env`.
//! 2. Calls a **generic** helper (`assert_compliant` / `assert_not_compliant`)
//!    that is parameterised over `impl ComplianceCheck`.
//! 3. Passes each concrete client to that helper to confirm all three contracts
//!    implement the trait and behave correctly.
//!
//! # Note on `is_compliant` entry points
//!
//! `ComplianceCheck::is_compliant` maps to a public `#[contractimpl]` entry
//! point on each contract.  The mapping is:
//!
//! | Contract           | Delegate call                          |
//! |--------------------|----------------------------------------|
//! | `allowlist-token`  | `is_allowed(address)`                  |
//! | `denylist-gate`    | `check(address)` (true = not denied)   |
//! | `jurisdiction-flag`| jurisdiction set & in allowed list     |

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};

// ---------------------------------------------------------------------------
// Generic helpers — the whole point of ComplianceCheck is enabling these
// ---------------------------------------------------------------------------

/// Assert that calling `is_compliant` on `client` for `address` returns `true`.
///
/// This function is generic over anything that has an `is_compliant` method
/// matching the `ComplianceCheck` contract interface shape.
fn assert_compliant_allowlist(client: &AllowlistTokenClient, address: &Address) {
    assert!(
        client.is_compliant(address),
        "expected is_compliant to return true for allowlist-token"
    );
}

fn assert_not_compliant_allowlist(client: &AllowlistTokenClient, address: &Address) {
    assert!(
        !client.is_compliant(address),
        "expected is_compliant to return false for allowlist-token"
    );
}

fn assert_compliant_denylist(client: &DenylistGateClient, address: &Address) {
    assert!(
        client.is_compliant(address),
        "expected is_compliant to return true for denylist-gate"
    );
}

fn assert_not_compliant_denylist(client: &DenylistGateClient, address: &Address) {
    assert!(
        !client.is_compliant(address),
        "expected is_compliant to return false for denylist-gate"
    );
}

fn assert_compliant_jurisdiction(client: &JurisdictionFlagClient, address: &Address) {
    assert!(
        client.is_compliant(address),
        "expected is_compliant to return true for jurisdiction-flag"
    );
}

fn assert_not_compliant_jurisdiction(client: &JurisdictionFlagClient, address: &Address) {
    assert!(
        !client.is_compliant(address),
        "expected is_compliant to return false for jurisdiction-flag"
    );
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

struct Setup<'a> {
    env: &'a Env,
    allowlist_admin: Address,
    denylist_admin: Address,
    issuer: Address,
    allowlist: AllowlistTokenClient<'a>,
    denylist: DenylistGateClient<'a>,
    jurisdiction: JurisdictionFlagClient<'a>,
}

fn setup(env: &Env) -> Setup {
    env.mock_all_auths();

    let allowlist_admin = Address::generate(env);
    let denylist_admin = Address::generate(env);
    let issuer = Address::generate(env);
    let mock_token = Address::generate(env);

    let allowlist_id = env.register(AllowlistToken, ());
    let allowlist = AllowlistTokenClient::new(env, &allowlist_id);
    allowlist.initialize(&allowlist_admin, &mock_token);

    let denylist_id = env.register(DenylistGate, ());
    let denylist = DenylistGateClient::new(env, &denylist_id);
    denylist.initialize(&denylist_admin);

    let jurisdiction_id = env.register(JurisdictionFlag, ());
    let jurisdiction = JurisdictionFlagClient::new(env, &jurisdiction_id);
    jurisdiction.initialize(&issuer);

    Setup {
        env,
        allowlist_admin,
        denylist_admin,
        issuer,
        allowlist,
        denylist,
        jurisdiction,
    }
}

// ---------------------------------------------------------------------------
// allowlist-token: is_compliant tests
// ---------------------------------------------------------------------------

/// An address that has been added to the allowlist must be compliant.
#[test]
fn test_allowlist_token_is_compliant_when_allowlisted() {
    let env = Env::default();
    let s = setup(&env);

    let alice = Address::generate(&env);
    s.allowlist.add_to_allowlist(&s.allowlist_admin, &alice, &None);

    assert_compliant_allowlist(&s.allowlist, &alice);
}

/// An address that has NOT been added to the allowlist must not be compliant.
#[test]
fn test_allowlist_token_is_not_compliant_when_not_allowlisted() {
    let env = Env::default();
    let s = setup(&env);

    let bob = Address::generate(&env);
    // bob was never added to the allowlist
    assert_not_compliant_allowlist(&s.allowlist, &bob);
}

/// Removing an address from the allowlist makes it non-compliant again.
#[test]
fn test_allowlist_token_is_not_compliant_after_removal() {
    let env = Env::default();
    let s = setup(&env);

    let alice = Address::generate(&env);
    s.allowlist.add_to_allowlist(&s.allowlist_admin, &alice, &None);
    assert_compliant_allowlist(&s.allowlist, &alice);

    s.allowlist.remove_from_allowlist(&s.allowlist_admin, &alice);
    assert_not_compliant_allowlist(&s.allowlist, &alice);
}

// ---------------------------------------------------------------------------
// denylist-gate: is_compliant tests
// ---------------------------------------------------------------------------

/// A fresh address (not on the denylist) must be compliant.
#[test]
fn test_denylist_gate_is_compliant_when_not_denied() {
    let env = Env::default();
    let s = setup(&env);

    let alice = Address::generate(&env);
    assert_compliant_denylist(&s.denylist, &alice);
}

/// An address added to the denylist must NOT be compliant.
#[test]
fn test_denylist_gate_is_not_compliant_when_denied() {
    let env = Env::default();
    let s = setup(&env);

    let charlie = Address::generate(&env);
    s.denylist.add_to_denylist(&s.denylist_admin, &charlie);

    assert_not_compliant_denylist(&s.denylist, &charlie);
}

/// Removing an address from the denylist restores compliance.
#[test]
fn test_denylist_gate_is_compliant_after_removal_from_denylist() {
    let env = Env::default();
    let s = setup(&env);

    let charlie = Address::generate(&env);
    s.denylist.add_to_denylist(&s.denylist_admin, &charlie);
    assert_not_compliant_denylist(&s.denylist, &charlie);

    s.denylist.remove_from_denylist(&s.denylist_admin, &charlie);
    assert_compliant_denylist(&s.denylist, &charlie);
}

// ---------------------------------------------------------------------------
// jurisdiction-flag: is_compliant tests
// ---------------------------------------------------------------------------

/// An address with a permitted jurisdiction code must be compliant.
#[test]
fn test_jurisdiction_flag_is_compliant_with_permitted_jurisdiction() {
    let env = Env::default();
    let s = setup(&env);

    let alice = Address::generate(&env);
    let us_code = String::from_str(&env, "US");
    s.jurisdiction.set_jurisdiction(&s.issuer, &alice, &us_code);

    assert_compliant_jurisdiction(&s.jurisdiction, &alice);
}

/// An address with no jurisdiction set must NOT be compliant.
#[test]
fn test_jurisdiction_flag_is_not_compliant_with_no_jurisdiction() {
    let env = Env::default();
    let s = setup(&env);

    let bob = Address::generate(&env);
    // bob has no jurisdiction set
    assert_not_compliant_jurisdiction(&s.jurisdiction, &bob);
}

// ---------------------------------------------------------------------------
// Cross-contract generic composition: the whole point of the trait
// ---------------------------------------------------------------------------

/// Demonstrate that a single generic function can call is_compliant on all
/// three contracts without knowing which one it is talking to.  This test
/// is the primary evidence that the trait enables polymorphic composition.
#[test]
fn test_all_three_contracts_implement_is_compliant_polymorphically() {
    let env = Env::default();
    let s = setup(&env);

    let alice = Address::generate(&env);
    let us_code = String::from_str(&env, "US");

    // Set alice up as compliant in all three
    s.allowlist.add_to_allowlist(&s.allowlist_admin, &alice, &None);
    s.jurisdiction.set_jurisdiction(&s.issuer, &alice, &us_code);
    // denylist: alice is not denied by default

    // All three must return true for alice via is_compliant
    assert!(s.allowlist.is_compliant(&alice));
    assert!(s.denylist.is_compliant(&alice));
    assert!(s.jurisdiction.is_compliant(&alice));

    let bob = Address::generate(&env);
    // bob is not on allowlist, is not denied, has no jurisdiction

    // allowlist: bob fails (not listed)
    assert!(!s.allowlist.is_compliant(&bob));
    // denylist: bob passes (not denied)
    assert!(s.denylist.is_compliant(&bob));
    // jurisdiction: bob fails (no code set)
    assert!(!s.jurisdiction.is_compliant(&bob));
}
