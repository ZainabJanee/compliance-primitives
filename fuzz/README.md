# Fuzz harnesses

This repo has two fuzzing layers:

1. **Seeded-PRNG sequence fuzzers** inside each contract's `#[cfg(test)]`
   tree. They run on stable as part of `cargo test` and print the failing
   `seed` for replay.
2. **`cargo-fuzz` (libFuzzer) project** in this directory (`fuzz/`). It
   drives the same denylist-gate and jurisdiction-flag invariants from raw
   bytes, with coverage guidance. It needs nightly and is not part of the
   root workspace or CI.

## cargo-fuzz targets

| Target                                         | Source harness                                                |
| ---------------------------------------------- | ------------------------------------------------------------- |
| `denylist_gate_sequences`                      | `contracts/denylist-gate/src/fuzz_test.rs` (`fuzz_denylist_sequences`) |
| `jurisdiction_set_get_sequences`               | `contracts/jurisdiction-flag/src/fuzz.rs` (`fuzz_jurisdiction_set_get_sequences`) |
| `jurisdiction_multicode_add_remove_sequences`  | `contracts/jurisdiction-flag/src/fuzz.rs` (`fuzz_jurisdiction_multicode_add_remove_sequences`) |

Setup (once):

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
```

Run a target (from the repo root):

```sh
cargo +nightly fuzz run denylist_gate_sequences -- -runs=1000
cargo +nightly fuzz run jurisdiction_set_get_sequences -- -runs=1000
cargo +nightly fuzz run jurisdiction_multicode_add_remove_sequences -- -runs=1000
```

Run until stopped (no `-runs` limit) or add `-max_total_time=300` for a
time-boxed campaign. Crashing inputs are written to `fuzz/artifacts/<target>/`;
replay one with `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`.
Corpus lives in `fuzz/corpus/<target>/`.

### Input encoding

Each target reads the fuzz input as a sequence of fixed-width chunks, and
each chunk is one contract operation. Chunks beyond the per-target op cap
(64) are ignored. Empty input is a no-op.

- `denylist_gate_sequences`: 2 bytes per op. `b0 % 3` = add / remove / check,
  `b1 % 5` = address index.
- `jurisdiction_set_get_sequences`: 2 bytes per op. `b0 % 5` = address index,
  `b1 % 7` = code index. Calls `set_jurisdiction` only.
- `jurisdiction_multicode_add_remove_sequences`: 2 bytes per op. `b0 % 7` =
  code index, low bit of `b1` = add (0) / remove (1).

### Extending a target

1. Add the new operation as a byte-decoded arm in the target's loop, keeping
   the chunk layout documented above (append bytes if you must change width,
   and update this section).
2. Mirror the operation in the model (`denied`, `model`, ...) and add an
   assertion that the contract agrees with the model.
3. Add a matching `[[bin]]` entry to `fuzz/Cargo.toml` for any new target,
   and a row to the table above.
4. Smoke-test with `cargo +nightly fuzz run <target> -- -runs=1000`.

The fuzz crate depends on `denylist-gate` and `jurisdiction-flag` by path, so
the contract's public client API is what gets exercised. The in-crate
harnesses stay the place for cases that need private items.

## jurisdiction-flag (`#87`)

Harness: `contracts/jurisdiction-flag/src/fuzz.rs`

Invariants checked after every random `set_jurisdiction` sequence:

1. **Last write wins** — `get_jurisdiction(addr)` equals the most recently
   set code for that address (or `None` if never set).
2. **Permission consistency** — `is_permitted_jurisdiction(addr, allowed)`
   matches `get_jurisdiction(addr)` membership in the fuzzer's fixed
   `allowed_codes` list.

### Short run (default, also in `cargo test`)

```sh
cargo test -p jurisdiction-flag fuzz_jurisdiction_set_get_sequences -- --nocapture
```

Defaults: `FUZZ_ITERATIONS=128`, `FUZZ_OPS=24`.

### Periodic longer campaign (not in CI)

```sh
FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
  cargo test -p jurisdiction-flag fuzz_jurisdiction_set_get_sequences -- --nocapture
```

Run this after non-trivial changes to `set_jurisdiction` /
`get_jurisdiction` / `is_permitted_jurisdiction`, or as part of a release
checklist. Failures print the failing `seed` so the sequence is reproducible.

### Expiring jurisdiction entries (#397)

Boundary fuzz coverage for `set_jurisdiction_until` is blocked: the current
contract has no such entrypoint, `valid_until` value, or expiry storage. Once
expiry semantics are implemented, add checks for `valid_until` equal to zero,
the current ledger sequence, values near `u32::MAX`, and randomized values;
assert that only the documented `Error` results are returned for invalid
boundaries. See #397 for the requested follow-up.

## policy-engine (`#234`)

Harness: `contracts/policy-engine/src/fuzz.rs`

Randomly builds combinator trees (up to `MAX_CHECKS=16` entries) using a
mix of `AddDenylist`, `AddJurisdiction`, and `RemoveCheck` mutations, arms
addresses on the denylist or with jurisdiction codes, then calls `evaluate`
for a random `(from, to)` pair.

Invariants checked after every random sequence:

1. **No panic** — `add_check`, `remove_check`, and `evaluate` must never
   panic regardless of the input sequence.
2. **MaxDepthExceeded is returned** — `add_check` returns
   `Err(Error::MaxDepthExceeded)` when the list is at capacity; it does not
   panic and does not silently truncate.
3. **All semantics oracle** — with `CombineOp::All`, `evaluate` returns
   `true` iff every registered check passes for both `from` and `to`.
4. **Any semantics oracle** — with `CombineOp::Any`, `evaluate` returns
   `true` iff at least one registered check passes for both `from` and `to`.

### Short run (default, also in `cargo test`)

```sh
cargo test -p policy-engine fuzz_policy_engine_tree -- --nocapture
```

Defaults: `FUZZ_ITERATIONS=128`, `FUZZ_OPS=24`.

### Periodic longer campaign (not in CI)

```sh
FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
  cargo test -p policy-engine fuzz_policy_engine_tree -- --nocapture
```

Run this after non-trivial changes to `add_check` / `remove_check` /
`evaluate`, or as part of a release checklist. Failures print the failing
`seed` so the exact failing sequence is fully reproducible.

## allowlist-token (`#22`)

Harness: `contracts/allowlist-token/src/fuzz.rs`

**Why a seeded-PRNG loop instead of `cargo-fuzz` or `proptest`:** it matches
the other harnesses in this repo, runs on the stable toolchain as part of
`cargo test` with no extra dependencies, and prints the failing `seed` so any
counterexample replays exactly. `cargo-fuzz` would need nightly plus a
separate workspace that re-creates the Soroban test `Env` from raw bytes.

Each iteration puts the contract in front of a `CountingToken` double (it
counts how many transfers reach it) and applies a random sequence of
`add_to_allowlist` (with and without an `expiration_ledger`),
`remove_from_allowlist`, ledger advances (so expiries actually lapse),
`pause`/`unpause`, and `transfer` calls with random parties and amounts
(including negative and `i128` extremes).

Invariants checked after every step:

1. **Forward only when both parties are allowlisted** — `transfer` reaches
   the underlying token (and returns `Ok(true)`) only when the contract is
   unpaused, `amount >= 0`, and both `from` and `to` are allowlisted *at call
   time* (entry present and not past its `expiration_ledger`). In every other
   case the underlying token's call counter must not change.
2. **Error mapping** — negative amount → `InvalidInput`, paused →
   `ContractPaused`, a non-allowlisted party → `Ok(false)`.
3. **Model agreement** — `is_allowed(addr)` matches the harness's model for
   every address.

### Short run (default, also in `cargo test`)

```sh
cargo test -p allowlist-token fuzz_transfer_only_forwards_when_both_allowlisted -- --nocapture
```

Defaults: `FUZZ_ITERATIONS=128`, `FUZZ_OPS=24`. This bounded run is part of
the regular workspace `cargo test`, so it runs in CI.

### Periodic longer campaign (not in CI)

```sh
FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
  cargo test -p allowlist-token fuzz_transfer_only_forwards_when_both_allowlisted -- --nocapture
```

Run this after non-trivial changes to `transfer`, `is_allowed`,
`add_to_allowlist`, or `remove_from_allowlist`, or as part of a release
checklist.

## compliance-aggregator (`#380`)

Harness: `contracts/compliance-aggregator/src/fuzz.rs`

Each iteration randomly registers the denylist-gate, the jurisdiction-flag, or
both, applies a random sequence of denylist adds/removes and jurisdiction
sets over a small address pool, then calls `check_all` on a random batch.

Invariants checked for every returned `AddressCheckResult`:

1. **all_passed consistency** — `all_passed` is `true` iff every individual
   `CheckResult.passed` is `true`.
2. **Shape** — one result per input address in input order, with one
   `CheckResult` per registered check (denylist first, then jurisdiction).
3. **Model agreement** — each `CheckResult.passed` matches the harness model
   of the denylist / jurisdiction state.

```sh
cargo test -p compliance-aggregator fuzz_check_all_combinations -- --nocapture

FUZZ_ITERATIONS=2000 FUZZ_OPS=64 \
  cargo test -p compliance-aggregator fuzz_check_all_combinations -- --nocapture
```

Defaults: `FUZZ_ITERATIONS=128`, `FUZZ_OPS=24`. Failures print the failing
`seed`.
