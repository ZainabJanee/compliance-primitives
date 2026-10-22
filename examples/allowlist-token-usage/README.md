# allowlist-token-usage

An end-to-end walkthrough of `allowlist-token`: from deploying the contract to
observing a successful transfer and a blocked one (with the `Blocked` event).
It ships two ways:

- `run.sh` — a shell script driving the contract from the Stellar CLI.
- `src/lib.rs` — a Rust crate exposing `walkthrough()`, which runs the same
  lifecycle in-process through a generated `AllowlistClient`. It returns an
  `Outcome` so callers can assert on the blocked and allowed transfers and on
  `is_allowed` after removal.

## What this covers

| Step | Description |
|------|-------------|
| Deploy underlying token | Uses the Stellar Asset Contract (SAC) so no extra WASM build is needed |
| Deploy & initialize `allowlist-token` | Points it at the underlying token |
| `add_to_allowlist` (Alice only) | Alice is now allowed; Bob is not |
| `transfer` Alice → Bob (blocked) | Returns `false`; `Blocked` event emitted; underlying token never touched |
| `add_to_allowlist` (Bob) | Bob joins the allowlist |
| `transfer` Alice → Bob (success) | Returns `true`; underlying SAC forwards the real balance move |

## How to run

### Prerequisites

1. **Stellar CLI v22+** — [install guide](https://developers.stellar.org/docs/tools/stellar-cli)
2. A **local Soroban network** running:
   ```sh
   stellar network start local
   ```
   (See the [repo README Quick start](../../README.md#quick-start) for details.)
3. Three funded local identities:
   ```sh
   stellar keys generate --default-seed admin
   stellar keys generate --default-seed alice
   stellar keys generate --default-seed bob
   stellar keys fund admin --network local
   stellar keys fund alice --network local
   stellar keys fund bob   --network local
   ```
4. Contracts built:
   ```sh
   stellar contract build
   ```

### Run

```sh
bash examples/allowlist-token-usage/run.sh
```

The script prints contract IDs, return values, and raw event JSON at each step.

## Key observations

- **Blocked transfer** (`Step 4`): `transfer` returns `false` and emits a
  `Blocked { from, to, amount }` event. Returning `Ok(false)` instead of
  `Err(...)` is intentional — a Soroban invocation that returns a contract error
  rolls back all its events too, so the audit event would be lost. The `false`
  return signals the block to the caller while preserving the on-chain record.
- **Successful transfer** (`Step 6`): `transfer` returns `true` and the
  underlying token contract's balance is updated. No `Blocked` event is emitted.
