//! Lightweight seeded-PRNG fuzzer for `check_all` check-kind combinations.
//!
//! Each iteration randomly registers a denylist-gate, a jurisdiction-flag, or
//! both, arms a pool of addresses with random denylist / jurisdiction state,
//! then calls `check_all` over a random batch and asserts that
//! `AddressCheckResult.all_passed` is consistent with the individual
//! `CheckResult`s and with a plain model of the underlying state.
//!
//! ```sh
//! cargo test -p compliance-aggregator fuzz_check_all_combinations -- --nocapture
//!
//! FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
//!   cargo test -p compliance-aggregator fuzz_check_all_combinations -- --nocapture
//! ```
//!
//! Failures print the failing `seed` so the configuration is reproducible.

extern crate std;

use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Env, String, Vec};

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

#[test]
fn fuzz_check_all_combinations() {
    let iterations: u32 = std::env::var("FUZZ_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(128);
    let ops_per_iter: u32 = std::env::var("FUZZ_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);

    let code_strs = ["US", "CA", "GB", "DE", "JP"];
    let allowed_strs = ["US", "CA", "GB"];
    const POOL: usize = 6;

    for seed in 1..=iterations {
        let mut rng = seed.wrapping_mul(0x9E37_79B1) | 1;
        let env = Env::default();
        env.mock_all_auths();

        let gate_admin = Address::generate(&env);
        let gate_id = env.register(DenylistGate, ());
        let gate = DenylistGateClient::new(&env, &gate_id);
        gate.initialize(&gate_admin);

        let flag_issuer = Address::generate(&env);
        let flag_id = env.register(JurisdictionFlag, ());
        let flag = JurisdictionFlagClient::new(&env, &flag_id);
        flag.initialize(&flag_issuer);

        // Random check-kind configuration; at least one check is registered.
        let (use_gate, use_flag) = match next_usize(&mut rng, 3) {
            0 => (true, false),
            1 => (false, true),
            _ => (true, true),
        };

        let agg_admin = Address::generate(&env);
        let agg_id = env.register(ComplianceAggregator, ());
        let agg = ComplianceAggregatorClient::new(&env, &agg_id);
        agg.initialize(
            &agg_admin,
            &use_gate.then(|| gate_id.clone()),
            &use_flag.then(|| flag_id.clone()),
            &None,
        );

        let pool: [Address; POOL] = core::array::from_fn(|_| Address::generate(&env));
        let mut denylisted = [false; POOL];
        let mut jurisdiction: [Option<usize>; POOL] = [None; POOL];

        for _ in 0..ops_per_iter {
            let i = next_usize(&mut rng, POOL);
            match next_usize(&mut rng, 3) {
                0 => {
                    gate.add_to_denylist(&gate_admin, &pool[i]);
                    denylisted[i] = true;
                }
                1 => {
                    gate.remove_from_denylist(&gate_admin, &pool[i]);
                    denylisted[i] = false;
                }
                _ => {
                    let c = next_usize(&mut rng, code_strs.len());
                    flag.set_jurisdiction(
                        &flag_issuer,
                        &pool[i],
                        &String::from_str(&env, code_strs[c]),
                    );
                    jurisdiction[i] = Some(c);
                }
            }
        }

        let mut allowed: Vec<String> = Vec::new(&env);
        for s in allowed_strs {
            allowed.push_back(String::from_str(&env, s));
        }

        // Random non-empty batch (duplicates allowed).
        let batch_len = 1 + next_usize(&mut rng, 8);
        let mut picks = std::vec::Vec::new();
        let mut addresses: Vec<Address> = Vec::new(&env);
        for _ in 0..batch_len {
            let i = next_usize(&mut rng, POOL);
            picks.push(i);
            addresses.push_back(pool[i].clone());
        }

        let results = agg.check_all(&addresses, &allowed);
        assert_eq!(results.len() as usize, batch_len, "seed {seed}: batch length");

        for (n, &i) in picks.iter().enumerate() {
            let r = results.get(n as u32).unwrap();
            assert_eq!(r.address, pool[i], "seed {seed}: order preserved at {n}");

            let expected_checks = use_gate as u32 + use_flag as u32;
            assert_eq!(
                r.checks.len(),
                expected_checks,
                "seed {seed}: check count at {n}"
            );

            // all_passed iff every individual check passed.
            let every = r.checks.iter().all(|c| c.passed);
            assert_eq!(r.all_passed, every, "seed {seed}: all_passed vs checks at {n}");

            // Each registered check appears once, in registration order,
            // and agrees with the model.
            let mut idx = 0;
            if use_gate {
                let c = r.checks.get(idx).unwrap();
                assert_eq!(c.check, CheckKind::Denylist, "seed {seed}: kind at {n}");
                assert_eq!(c.passed, !denylisted[i], "seed {seed}: denylist at {n}");
                idx += 1;
            }
            if use_flag {
                let c = r.checks.get(idx).unwrap();
                assert_eq!(c.check, CheckKind::Jurisdiction, "seed {seed}: kind at {n}");
                let permitted = jurisdiction[i]
                    .map(|c| allowed_strs.contains(&code_strs[c]))
                    .unwrap_or(false);
                assert_eq!(c.passed, permitted, "seed {seed}: jurisdiction at {n}");
            }
        }
    }
}
