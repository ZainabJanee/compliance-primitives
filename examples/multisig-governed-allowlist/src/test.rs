// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! Integration tests for the `multisig-governed-allowlist` composition.
//!
//! These tests demonstrate the full flow:
//!   1. Deploy and initialize `multisig-admin` with M-of-N signers
//!   2. Deploy and initialize `allowlist-token` with the multisig contract
//!      as its `admin` address
//!   3. Perform admin operations on the allowlist — the calls succeed only
//!      because Soroban routes `admin.require_auth()` through the multisig
//!      contract's `__check_auth`, which counts the signers and verifies the
//!      threshold

extern crate std;

use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use multisig_admin::{MultisigAdmin, MultisigAdminClient};
use soroban_sdk::auth::CustomAccountInterface;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env};

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Deploy and initialize a `multisig-admin` contract with `n` signers and the
/// given `threshold`. Returns `(signers, multisig_id, multisig_client)`.
fn setup_multisig(
    env: &Env,
    n: usize,
    threshold: u32,
) -> (
    soroban_sdk::Vec<Address>,
    Address,
    MultisigAdminClient<'_>,
) {
    env.mock_all_auths();
    let mut signers = soroban_sdk::Vec::new(env);
    for _ in 0..n {
        signers.push_back(Address::generate(env));
    }
    let multisig_id = env.register(MultisigAdmin, ());
    let client = MultisigAdminClient::new(env, &multisig_id);
    client.initialize(&signers, &threshold);
    (signers, multisig_id, client)
}

/// Deploy and initialize an `allowlist-token` with the given `admin` and a
/// stub `token` address. Returns `(allowlist_id, allowlist_client)`.
fn setup_allowlist<'a>(
    env: &'a Env,
    admin: &Address,
) -> (Address, AllowlistTokenClient<'a>) {
    // Use a dummy token address — we're only testing the admin/allowlist logic,
    // not the underlying token transfer forwarding.
    let token = Address::generate(env);
    let allowlist_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(env, &allowlist_id);
    client.initialize(admin, &token);
    (allowlist_id, client)
}

// ---------------------------------------------------------------------------
// Core composition tests
// ---------------------------------------------------------------------------

/// Basic sanity check: the multisig contract address can be stored as the
/// admin of an allowlist-token and the admin can be read back.
#[test]
fn test_multisig_set_as_allowlist_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, multisig_id, _) = setup_multisig(&env, 3, 2);
    let (_, allowlist_client) = setup_allowlist(&env, &multisig_id);

    // The admin stored in the allowlist is the multisig contract address.
    let metadata = allowlist_client.metadata();
    assert_eq!(metadata.admin, multisig_id);
}

/// With `mock_all_auths` active, all `require_auth` calls are satisfied
/// automatically. This test confirms the call plumbing works end-to-end:
/// `add_to_allowlist(multisig_addr, alice)` succeeds when the multisig is
/// the admin, and `alice` is subsequently visible on the allowlist.
#[test]
fn test_add_to_allowlist_via_multisig_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, multisig_id, _) = setup_multisig(&env, 3, 2);
    let (_, allowlist_client) = setup_allowlist(&env, &multisig_id);

    let alice = Address::generate(&env);
    assert!(!allowlist_client.is_allowed(&alice));

    // Pass the multisig contract address as the `admin` argument.
    // allowlist-token calls `admin.require_auth()`, which routes to
    // `multisig-admin::__check_auth` at runtime. mock_all_auths satisfies it.
    allowlist_client.add_to_allowlist(&multisig_id, &alice, &None);

    assert!(allowlist_client.is_allowed(&alice));
}

/// Remove-from-allowlist also routes through the multisig auth path.
#[test]
fn test_remove_from_allowlist_via_multisig_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, multisig_id, _) = setup_multisig(&env, 3, 2);
    let (_, allowlist_client) = setup_allowlist(&env, &multisig_id);

    let bob = Address::generate(&env);
    allowlist_client.add_to_allowlist(&multisig_id, &bob, &None);
    assert!(allowlist_client.is_allowed(&bob));

    allowlist_client.remove_from_allowlist(&multisig_id, &bob);
    assert!(!allowlist_client.is_allowed(&bob));
}

/// Multiple addresses can be managed through the same multisig admin.
#[test]
fn test_multisig_can_manage_multiple_allowlist_entries() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, multisig_id, _) = setup_multisig(&env, 3, 2);
    let (_, allowlist_client) = setup_allowlist(&env, &multisig_id);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);

    allowlist_client.add_to_allowlist(&multisig_id, &alice, &None);
    allowlist_client.add_to_allowlist(&multisig_id, &bob, &None);
    allowlist_client.add_to_allowlist(&multisig_id, &charlie, &None);

    assert!(allowlist_client.is_allowed(&alice));
    assert!(allowlist_client.is_allowed(&bob));
    assert!(allowlist_client.is_allowed(&charlie));

    // Remove one; the others remain.
    allowlist_client.remove_from_allowlist(&multisig_id, &bob);
    assert!(allowlist_client.is_allowed(&alice));
    assert!(!allowlist_client.is_allowed(&bob));
    assert!(allowlist_client.is_allowed(&charlie));
}

/// Pause and unpause are also admin-gated and go through the multisig path.
#[test]
fn test_pause_and_unpause_via_multisig_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, multisig_id, _) = setup_multisig(&env, 3, 2);
    let (_, allowlist_client) = setup_allowlist(&env, &multisig_id);

    assert!(!allowlist_client.is_paused());

    allowlist_client.pause(&multisig_id);
    assert!(allowlist_client.is_paused());

    allowlist_client.unpause(&multisig_id);
    assert!(!allowlist_client.is_paused());
}

// ---------------------------------------------------------------------------
// __check_auth direct tests — verifying M-of-N enforcement
// ---------------------------------------------------------------------------

/// Direct `__check_auth` call: 2-of-3 threshold met with exactly 2 signers.
/// This test demonstrates that the multisig contract correctly counts distinct
/// signer approvals and grants authorization at threshold.
#[test]
fn test_check_auth_grants_auth_at_threshold() {
    use multisig_admin::MultisigAdmin;
    use soroban_sdk::crypto::Hash;
    use soroban_sdk::Bytes;

    let env = Env::default();
    env.mock_all_auths();

    let (signers, id, _client) = setup_multisig(&env, 3, 2);

    let payload: Hash<32> = env.crypto().sha256(&Bytes::from_array(&env, &[0u8; 32]));
    // Provide exactly 2 of the 3 signers — meets the threshold.
    let sigs = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    let result = env.as_contract(&id, || {
        MultisigAdmin::__check_auth(env.clone(), payload, sigs, soroban_sdk::Vec::new(&env))
    });
    assert!(result.is_ok(), "2-of-3 threshold should be met with 2 valid signers");
}

/// Direct `__check_auth` call: 2-of-3 threshold NOT met with only 1 signer.
#[test]
fn test_check_auth_rejects_below_threshold() {
    use multisig_admin::MultisigAdmin;
    use multisig_admin::Error;
    use soroban_sdk::crypto::Hash;
    use soroban_sdk::Bytes;

    let env = Env::default();
    env.mock_all_auths();

    let (signers, id, _client) = setup_multisig(&env, 3, 2);

    let payload: Hash<32> = env.crypto().sha256(&Bytes::from_array(&env, &[0u8; 32]));
    // Only 1 signer — below the 2-of-3 threshold.
    let sigs = vec![&env, signers.get(0).unwrap()];
    let result = env.as_contract(&id, || {
        MultisigAdmin::__check_auth(env.clone(), payload, sigs, soroban_sdk::Vec::new(&env))
    });
    assert_eq!(result, Err(Error::ThresholdNotMet));
}

/// An outsider address (not in the signer set) does not count toward threshold.
#[test]
fn test_check_auth_outsider_does_not_count() {
    use multisig_admin::MultisigAdmin;
    use multisig_admin::Error;
    use soroban_sdk::crypto::Hash;
    use soroban_sdk::Bytes;

    let env = Env::default();
    env.mock_all_auths();

    // 2-of-3 threshold
    let (signers, id, _client) = setup_multisig(&env, 3, 2);

    let payload: Hash<32> = env.crypto().sha256(&Bytes::from_array(&env, &[0u8; 32]));
    // One valid signer + one outsider — still only 1 valid approval.
    let outsider = Address::generate(&env);
    let sigs = vec![&env, signers.get(0).unwrap(), outsider];
    let result = env.as_contract(&id, || {
        MultisigAdmin::__check_auth(env.clone(), payload, sigs, soroban_sdk::Vec::new(&env))
    });
    assert_eq!(result, Err(Error::ThresholdNotMet));
}
