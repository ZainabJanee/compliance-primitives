//! # Allowlist Token Usage
//!
//! Drives `allowlist-token` end-to-end through a client generated from a trait:
//! initialize, add and remove allowlist entries, read `is_allowed`, and transfer.
//! The steps mirror `run.sh`, but run in-process from Rust.
//!
//! The client is declared as a trait rather than depending on the
//! `allowlist-token` crate, so this example has no binary coupling to it. This
//! is the same pattern used by `examples/denylist-gate-sep41`.
#![no_std]

use soroban_sdk::{contractclient, Address, Env};

// ---------------------------------------------------------------------------
// Cross-contract client for allowlist-token (trait-only, no binary coupling).
// ---------------------------------------------------------------------------

#[contractclient(name = "AllowlistClient")]
pub trait AllowlistTokenInterface {
    fn initialize(env: Env, admin: Address, token: Address);
    fn add_to_allowlist(env: Env, admin: Address, address: Address);
    fn remove_from_allowlist(env: Env, admin: Address, address: Address);
    fn is_allowed(env: Env, address: Address) -> bool;
    fn transfer(env: Env, from: Address, to: Address, amount: i128) -> bool;
}

// ---------------------------------------------------------------------------
// Walkthrough
// ---------------------------------------------------------------------------

/// Results observed while driving the allowlist lifecycle.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Transfer before Bob is allowlisted: expected `false` (blocked).
    pub blocked_transfer: bool,
    /// Transfer after both parties are allowlisted: expected `true`.
    pub allowed_transfer: bool,
    /// `is_allowed(bob)` after removal: expected `false`.
    pub bob_allowed_after_removal: bool,
}

/// Deploys nothing itself: it expects `allowlist` to be a deployed
/// `allowlist-token` and `token` to be its underlying SEP-41 token.
///
/// Initializes the allowlist, allowlists `alice`, attempts a transfer to `bob`
/// (blocked), allowlists `bob`, transfers `amount` from `alice` to `bob`
/// (succeeds), then removes `bob` from the allowlist.
pub fn walkthrough(
    env: &Env,
    allowlist: &Address,
    token: &Address,
    admin: &Address,
    alice: &Address,
    bob: &Address,
    amount: i128,
) -> Outcome {
    let client = AllowlistClient::new(env, allowlist);

    client.initialize(admin, token);
    client.add_to_allowlist(admin, alice);

    let blocked_transfer = client.transfer(alice, bob, &amount);

    client.add_to_allowlist(admin, bob);
    let allowed_transfer = client.transfer(alice, bob, &amount);

    client.remove_from_allowlist(admin, bob);
    let bob_allowed_after_removal = client.is_allowed(bob);

    Outcome {
        blocked_transfer,
        allowed_transfer,
        bob_allowed_after_removal,
    }
}
