// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! Benchmark: resource cost of `add_to_allowlist` as the allowlist grows (#95).
//!
//! Every allowlist entry is its own persistent key (`DataKey::Allowed(addr)`),
//! so adding one address should cost the same regardless of how many
//! addresses are already allowlisted. This test pre-populates the allowlist
//! with 0 / 100 / 1,000 unrelated addresses, then measures the CPU
//! instructions and memory bytes charged for a single further
//! `add_to_allowlist` call and asserts the cost stays flat.
//!
//! Run with the measurements printed:
//!
//! ```text
//! cargo test -p allowlist-token --test add_to_allowlist_cost -- --nocapture
//! ```
//!
//! Results and interpretation are recorded in BENCHMARKS.md
//! ("add_to_allowlist cost vs. allowlist size").

use allowlist_token::{AllowlistToken, AllowlistTokenClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

/// Allowlist sizes (entries already present) to measure at.
const SIZES: [u32; 3] = [0, 100, 1_000];

/// Allowed growth in measured cost between the empty allowlist and the
/// largest size. The on-chain cost is O(1) per add (one key in the
/// footprint), but the local test host keeps every entry the test has ever
/// touched in one sorted in-memory map, so lookups there pick up a small
/// O(log n) term that doesn't exist on-chain. 10% matches the tolerance used
/// by the other budget checks (budget-baselines.toml) and comfortably covers
/// that term while still catching any real O(n) behavior, which would show
/// up as a many-fold increase at 1,000 entries.
const MAX_GROWTH: f64 = 1.10;

struct Measurement {
    size: u32,
    cpu: u64,
    memory: u64,
}

fn measure_add_at_size(size: u32) -> Measurement {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    // `add_to_allowlist` never touches the underlying token, so any address
    // will do here.
    let token = Address::generate(&env);
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &token);

    // Pre-populate outside the measured window; 1,000 adds would exceed the
    // default per-invocation test budget if it were never reset.
    let mut budget = env.cost_estimate().budget();
    budget.reset_unlimited();
    for _ in 0..size {
        client.add_to_allowlist(&admin, &Address::generate(&env), &None);
    }

    let target = Address::generate(&env);
    budget.reset_default();
    client.add_to_allowlist(&admin, &target, &None);
    let measurement = Measurement { size, cpu: budget.cpu_instruction_cost(), memory: budget.memory_bytes_cost() };

    assert!(client.is_allowed(&target));
    measurement
}

#[test]
fn bench_add_to_allowlist_cost_is_flat_as_allowlist_grows() {
    let results: std::vec::Vec<Measurement> = SIZES.iter().map(|&size| measure_add_at_size(size)).collect();

    let base = &results[0];
    println!("add_to_allowlist cost vs. allowlist size");
    println!("{:>10} {:>14} {:>14} {:>9} {:>9}", "entries", "cpu_insns", "mem_bytes", "cpu_x", "mem_x");
    for m in &results {
        println!(
            "{:>10} {:>14} {:>14} {:>9.3} {:>9.3}",
            m.size,
            m.cpu,
            m.memory,
            m.cpu as f64 / base.cpu as f64,
            m.memory as f64 / base.memory as f64,
        );
    }

    for m in &results[1..] {
        let cpu_limit = (base.cpu as f64 * MAX_GROWTH).ceil() as u64;
        let memory_limit = (base.memory as f64 * MAX_GROWTH).ceil() as u64;
        assert!(
            m.cpu <= cpu_limit && m.memory <= memory_limit,
            "add_to_allowlist cost grows with allowlist size: at {} entries it used \
             {} CPU insns / {} mem bytes vs {} / {} when empty (limit {} / {}). \
             This contradicts the O(1)-per-add design — file a bug/follow-up issue \
             rather than raising MAX_GROWTH.",
            m.size,
            m.cpu,
            m.memory,
            base.cpu,
            base.memory,
            cpu_limit,
            memory_limit,
        );
    }
}
