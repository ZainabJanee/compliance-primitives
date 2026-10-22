//! Composition-level fuzz harness for `circuit-breaker` interleaved with a
//! consumer token's transfer path.
//!
//! Resolves #465.
//!
//! # What is being fuzzed
//!
//! The `denylist-gate-consumer` example (`ExampleToken`) calls
//! `circuit-breaker::is_frozen()` and `denylist-gate::check()` before
//! mutating balances.  This harness tests the *composition* of those two
//! contracts under random interleaving of:
//!
//! - `freeze` / `unfreeze` calls on the circuit-breaker
//! - `add_to_denylist` / `remove_from_denylist` calls on the gate
//! - `transfer` calls on the consumer token
//!
//! # Invariants checked
//!
//! 1. **Frozen gate always blocks transfers** — while `is_frozen()` is `true`,
//!    every `transfer` call must return `Err(FrozenByBreaker)`, regardless of
//!    denylist state, balances, or call ordering.
//!
//! 2. **Unfrozen + denied still blocks** — when unfrozen but the sender or
//!    recipient is on the denylist, `transfer` returns `Err(DeniedByGate)`.
//!
//! 3. **Unfrozen + both clear allows transfer** — when unfrozen and neither
//!    party is denied, `transfer` with sufficient balance must succeed.
//!
//! 4. **Balances never mutate through a blocked transfer** — regardless of
//!    which error path is taken, balances must be unchanged after any
//!    rejected transfer.
//!
//! 5. **No panic** — no operation sequence may cause a panic inside the
//!    Soroban host, regardless of ordering.
//!
//! # Approach
//!
//! We use the same lightweight in-process deterministic PRNG pattern used
//! in `denylist-gate/src/fuzz_test.rs` and `policy-engine/src/fuzz.rs`.
//! No `cargo-fuzz`/libFuzzer dependency is needed; the harness runs as a
//! plain `#[test]` and is included in `cargo test`.
//!
//! # Running
//!
//! Default (CI-friendly, 500 seeds):
//! ```sh
//! cargo test -p denylist-gate-consumer fuzz_circuit_breaker_consumer_composition
//! ```
//!
//! Longer local campaign:
//! ```sh
//! FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
//!   cargo test -p denylist-gate-consumer fuzz_circuit_breaker_consumer_composition -- --nocapture
//! ```
//!
//! Failures print the failing `seed` so the sequence is fully reproducible.

extern crate std;

use super::*;
use circuit_breaker::{CircuitBreaker, CircuitBreakerClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

// ---------------------------------------------------------------------------
// Tiny deterministic xorshift32 PRNG — no external crate needed.
// ---------------------------------------------------------------------------

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        // Ensure state is never zero.
        Self(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn gen_usize(&mut self, upper: usize) -> usize {
        (self.next_u32() as usize) % upper
    }

    fn gen_bool(&mut self) -> bool {
        self.next_u32() & 1 == 0
    }
}

// ---------------------------------------------------------------------------
// Operations the fuzzer can issue
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Op {
    /// Call `circuit-breaker::freeze`.
    Freeze,
    /// Call `circuit-breaker::unfreeze`.
    Unfreeze,
    /// Call `denylist-gate::add_to_denylist` for a random address.
    Deny,
    /// Call `denylist-gate::remove_from_denylist` for a random address.
    Undeny,
    /// Call `ExampleToken::transfer` between two random addresses.
    Transfer,
}

const OPS: &[Op] = &[
    Op::Freeze,
    Op::Unfreeze,
    Op::Deny,
    Op::Undeny,
    Op::Transfer,
];

// ---------------------------------------------------------------------------
// Single fuzz iteration
// ---------------------------------------------------------------------------

fn run_sequence(seed: u32, ops_per_iter: u32) {
    let env = Env::default();
    env.mock_all_auths();

    // -- Deploy circuit-breaker --
    let breaker_admin = Address::generate(&env);
    let breaker_id = env.register(CircuitBreaker, ());
    let breaker = CircuitBreakerClient::new(&env, &breaker_id);
    breaker.initialize(&breaker_admin);

    // -- Deploy denylist-gate --
    let gate_admin = Address::generate(&env);
    let gate_id = env.register(DenylistGate, ());
    let gate = DenylistGateClient::new(&env, &gate_id);
    gate.initialize(&gate_admin);

    // -- Deploy consumer token wired to both --
    let token_id = env.register(ExampleToken, ());
    let token = ExampleTokenClient::new(&env, &token_id);
    token.initialize(&gate_id, &breaker_id);

    // Pool of addresses used as transfer parties and denylist targets.
    const NUM_ADDRS: usize = 4;
    let mut addresses = std::vec::Vec::new();
    for _ in 0..NUM_ADDRS {
        addresses.push(Address::generate(&env));
    }

    // Mint initial balances so transfers have something to move.
    const INITIAL_BALANCE: i128 = 10_000;
    for addr in &addresses {
        token.mint(addr, &INITIAL_BALANCE);
    }

    // Model state tracked in the test (mirrors what the contracts hold).
    let mut is_frozen = false;
    let mut denied = [false; NUM_ADDRS];

    let mut rng = Rng::new(seed);

    for _ in 0..ops_per_iter {
        let op = OPS[rng.gen_usize(OPS.len())];

        match op {
            Op::Freeze => {
                breaker.freeze(&breaker_admin, &None);
                is_frozen = true;
            }

            Op::Unfreeze => {
                breaker.unfreeze(&breaker_admin);
                is_frozen = false;
            }

            Op::Deny => {
                let idx = rng.gen_usize(NUM_ADDRS);
                gate.add_to_denylist(&gate_admin, &addresses[idx]);
                denied[idx] = true;
            }

            Op::Undeny => {
                let idx = rng.gen_usize(NUM_ADDRS);
                gate.remove_from_denylist(&gate_admin, &addresses[idx]);
                denied[idx] = false;
            }

            Op::Transfer => {
                let from_idx = rng.gen_usize(NUM_ADDRS);
                let to_idx = rng.gen_usize(NUM_ADDRS);
                let from = addresses[from_idx].clone();
                let to = addresses[to_idx].clone();

                // Snapshot balances before the call.
                let from_before = token.balance(&from);
                let to_before = token.balance(&to);

                // Use a small fixed amount so we don't exhaust balances.
                let amount: i128 = 1;

                let result = token.try_transfer(&from, &to, &amount);

                match result {
                    Ok(_) => {
                        // Invariant 1: must NOT succeed while frozen.
                        assert!(
                            !is_frozen,
                            "seed={seed}: transfer succeeded while frozen (from_idx={from_idx} to_idx={to_idx})"
                        );
                        // Invariant 2: must NOT succeed when sender is denied.
                        assert!(
                            !denied[from_idx],
                            "seed={seed}: transfer succeeded for denied sender (from_idx={from_idx})"
                        );
                        // Invariant 2: must NOT succeed when recipient is denied.
                        assert!(
                            !denied[to_idx],
                            "seed={seed}: transfer succeeded for denied recipient (to_idx={to_idx})"
                        );

                        // Balances must have moved correctly (unless from == to).
                        if from_idx == to_idx {
                            assert_eq!(
                                token.balance(&from),
                                from_before,
                                "seed={seed}: self-transfer mutated balance"
                            );
                        } else {
                            assert_eq!(
                                token.balance(&from),
                                from_before - amount,
                                "seed={seed}: sender balance not decremented"
                            );
                            assert_eq!(
                                token.balance(&to),
                                to_before + amount,
                                "seed={seed}: recipient balance not incremented"
                            );
                        }
                    }
                    Err(Ok(Error::FrozenByBreaker)) => {
                        // Invariant 1 (converse): must only fire while frozen.
                        assert!(
                            is_frozen,
                            "seed={seed}: FrozenByBreaker returned while NOT frozen (from_idx={from_idx})"
                        );
                        // Invariant 4: balances must be unchanged.
                        assert_eq!(
                            token.balance(&from),
                            from_before,
                            "seed={seed}: balance changed on FrozenByBreaker"
                        );
                        assert_eq!(
                            token.balance(&to),
                            to_before,
                            "seed={seed}: balance changed on FrozenByBreaker"
                        );
                    }
                    Err(Ok(Error::DeniedByGate)) => {
                        // Invariant 2 (converse): must only fire when unfrozen
                        // and at least one party is denied.
                        assert!(
                            !is_frozen,
                            "seed={seed}: DeniedByGate returned while frozen (expected FrozenByBreaker first)"
                        );
                        assert!(
                            denied[from_idx] || denied[to_idx],
                            "seed={seed}: DeniedByGate returned but neither party is denied \
                             (from_idx={from_idx} denied={} to_idx={to_idx} denied={})",
                            denied[from_idx],
                            denied[to_idx]
                        );
                        // Invariant 4: balances must be unchanged.
                        assert_eq!(
                            token.balance(&from),
                            from_before,
                            "seed={seed}: balance changed on DeniedByGate"
                        );
                        assert_eq!(
                            token.balance(&to),
                            to_before,
                            "seed={seed}: balance changed on DeniedByGate"
                        );
                    }
                    Err(Ok(Error::InsufficientBalance)) => {
                        // Balances can exhaust in a long run — that's fine.
                        // Balances must not have changed.
                        assert_eq!(token.balance(&from), from_before);
                        assert_eq!(token.balance(&to), to_before);
                    }
                    Err(err) => {
                        panic!("seed={seed}: unexpected error from transfer: {err:?}");
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test entry point
// ---------------------------------------------------------------------------

#[test]
fn fuzz_circuit_breaker_consumer_composition() {
    let iterations: u32 = std::env::var("FUZZ_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let ops_per_iter: u32 = std::env::var("FUZZ_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);

    for seed in 1..=iterations {
        run_sequence(seed, ops_per_iter);
    }
}
