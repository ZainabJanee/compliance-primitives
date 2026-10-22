//! Lightweight sequence fuzzer for the `allowlist-token::transfer` gate.
//!
//! ## Approach
//!
//! Same shape as the `jurisdiction-flag` (#87) and `policy-engine` (#234)
//! harnesses: a seeded xorshift PRNG loop inside the crate's test binary
//! rather than a `cargo-fuzz` target. `cargo-fuzz` / libFuzzer needs a
//! nightly toolchain and a separate fuzz workspace, and wiring the Soroban
//! host `Env`, `mock_all_auths`, and address generation through a raw
//! byte-buffer `fuzz_target!` duplicates most of the unit-test setup for a
//! `#![no_std]` crate. A deterministic PRNG loop needs no extra deps, runs on
//! stable as part of `cargo test`, and every failure prints its `seed` so the
//! exact sequence is reproducible.
//!
//! ## What it does
//!
//! Each iteration builds a fresh contract in front of a `CountingToken` (a
//! token double that records how many times `transfer` was forwarded to it)
//! and applies a random sequence of:
//!
//! - `add_to_allowlist` with a random expiry (`None`, or a ledger a little
//!   ahead of the current one),
//! - `remove_from_allowlist`,
//! - advancing the ledger sequence (so expiries actually fire),
//! - `pause` / `unpause`,
//! - `transfer` between two random addresses with a random amount
//!   (including negative amounts).
//!
//! A plain-Rust model mirrors the allowlist state.
//!
//! ## Invariants
//!
//! 1. **Forward iff both allowlisted** — a `transfer` call forwards to the
//!    underlying token (and returns `Ok(true)`) *only* when the contract is
//!    unpaused, `amount >= 0`, and both `from` and `to` are allowlisted at
//!    call time (entry present and not past its `expiration_ledger`).
//!    Otherwise the underlying token's call counter must not move.
//! 2. **Error mapping** — a negative amount is `InvalidInput`, a paused
//!    contract is `ContractPaused`, and a non-allowlisted party is
//!    `Ok(false)`.
//! 3. **`is_allowed` agrees with the model** for every address after every
//!    step.
//!
//! ## How to run
//!
//! Default short run (also covered by `cargo test -p allowlist-token`, and so
//! by the workspace-wide `cargo test` in CI):
//!
//! ```sh
//! cargo test -p allowlist-token fuzz_transfer_only_forwards_when_both_allowlisted -- --nocapture
//! ```
//!
//! Longer periodic campaign (not in CI — raise iterations / ops via env vars):
//!
//! ```sh
//! FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
//!   cargo test -p allowlist-token fuzz_transfer_only_forwards_when_both_allowlisted -- --nocapture
//! ```

extern crate std;

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{contract, contractimpl, Env, Symbol};

// ─── Token double ────────────────────────────────────────────────────────────

/// Underlying-token double that counts forwarded transfers so the harness can
/// tell exactly whether `allowlist-token` called through.
#[contract]
struct CountingToken;

#[contractimpl]
impl CountingToken {
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let key = Symbol::new(&env, "calls");
        let calls: u32 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(calls + 1));
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "last"), &(from, to, amount));
    }

    pub fn calls(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "calls"))
            .unwrap_or(0)
    }

    pub fn last_transfer(env: Env) -> Option<(Address, Address, i128)> {
        env.storage().instance().get(&Symbol::new(&env, "last"))
    }
}

// ─── PRNG ────────────────────────────────────────────────────────────────────

/// Tiny xorshift32 so we don't need an extra RNG crate in tests.
fn next_u32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = if x == 0 { 0x9E37_79B9 } else { x };
    *state
}

fn next_usize(state: &mut u32, upper: usize) -> usize {
    (next_u32(state) as usize) % upper
}

// ─── Model ───────────────────────────────────────────────────────────────────

/// `None` = no entry; `Some(None)` = allowlisted forever;
/// `Some(Some(n))` = allowlisted while ledger sequence `<= n`.
type ModelEntry = Option<Option<u32>>;

fn model_allowed(entry: ModelEntry, sequence: u32) -> bool {
    match entry {
        None => false,
        Some(None) => true,
        Some(Some(exp)) => sequence <= exp,
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ─── Harness ─────────────────────────────────────────────────────────────────

#[test]
fn fuzz_transfer_only_forwards_when_both_allowlisted() {
    let iterations = env_u32("FUZZ_ITERATIONS", 128);
    let ops_per_iter = env_u32("FUZZ_OPS", 24);

    // A mix of ordinary, boundary, and invalid amounts.
    let amounts: [i128; 7] = [0, 1, 500, i128::MAX, -1, -500, i128::MIN];

    for seed in 1..=iterations {
        let env = Env::default();
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().with_mut(|li| {
            li.sequence_number = 1_000;
            li.min_persistent_entry_ttl = 1_000_000;
            li.max_entry_ttl = 6_311_520;
        });

        let admin = Address::generate(&env);
        let token_id = env.register(CountingToken, ());
        let token = CountingTokenClient::new(&env, &token_id);
        let contract_id = env.register(AllowlistToken, ());
        let client = AllowlistTokenClient::new(&env, &contract_id);
        client.initialize(&admin, &token_id);

        let addresses: [Address; 4] = [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ];
        let mut model: [ModelEntry; 4] = [None; 4];
        let mut paused = false;
        let mut rng = seed;

        for step in 0..ops_per_iter {
            let sequence = env.ledger().sequence();
            match next_usize(&mut rng, 10) {
                // add_to_allowlist, with or without an expiry
                0..=2 => {
                    let i = next_usize(&mut rng, addresses.len());
                    let expiry = if next_usize(&mut rng, 2) == 0 {
                        None
                    } else {
                        Some(sequence + next_usize(&mut rng, 8) as u32)
                    };
                    client.add_to_allowlist(&admin, &addresses[i], &expiry);
                    model[i] = Some(expiry);
                }
                // remove_from_allowlist
                3 => {
                    let i = next_usize(&mut rng, addresses.len());
                    client.remove_from_allowlist(&admin, &addresses[i]);
                    model[i] = None;
                }
                // advance the ledger so expiries can lapse
                4 => {
                    let by = next_usize(&mut rng, 6) as u32;
                    env.ledger().with_mut(|li| li.sequence_number += by);
                }
                // toggle pause
                5 => {
                    if paused {
                        client.unpause(&admin);
                    } else {
                        client.pause(&admin);
                    }
                    paused = !paused;
                }
                // transfer
                _ => {
                    let from_i = next_usize(&mut rng, addresses.len());
                    let to_i = next_usize(&mut rng, addresses.len());
                    let amount = amounts[next_usize(&mut rng, amounts.len())];
                    let from = &addresses[from_i];
                    let to = &addresses[to_i];

                    let both_allowed = model_allowed(model[from_i], sequence)
                        && model_allowed(model[to_i], sequence);
                    let calls_before = token.calls();
                    let result = client.try_transfer(from, to, &amount);
                    let calls_after = token.calls();
                    let forwarded = calls_after != calls_before;

                    let ctx = || {
                        std::format!(
                            "seed={seed} step={step} from={from_i} to={to_i} amount={amount} \
                             paused={paused} seq={sequence} model={model:?}"
                        )
                    };

                    if amount < 0 {
                        assert_eq!(result, Err(Ok(Error::InvalidInput)), "{}", ctx());
                        assert!(!forwarded, "negative amount forwarded: {}", ctx());
                    } else if paused {
                        assert_eq!(result, Err(Ok(Error::ContractPaused)), "{}", ctx());
                        assert!(!forwarded, "paused transfer forwarded: {}", ctx());
                    } else if both_allowed {
                        assert_eq!(result, Ok(Ok(true)), "{}", ctx());
                        assert_eq!(calls_after, calls_before + 1, "{}", ctx());
                        assert_eq!(
                            token.last_transfer(),
                            Some((from.clone(), to.clone(), amount)),
                            "{}",
                            ctx()
                        );
                    } else {
                        assert_eq!(result, Ok(Ok(false)), "{}", ctx());
                        assert!(
                            !forwarded,
                            "transfer forwarded without both parties allowlisted: {}",
                            ctx()
                        );
                    }
                }
            }

            let sequence = env.ledger().sequence();
            for (i, address) in addresses.iter().enumerate() {
                assert_eq!(
                    client.is_allowed(address),
                    model_allowed(model[i], sequence),
                    "seed={seed} step={step} addr={i}: is_allowed disagrees with model {:?}",
                    model[i]
                );
            }
        }
    }
}
