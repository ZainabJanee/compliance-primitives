# Migration guide: moving from hand-rolled compliance to `compliance-primitives`

This guide walks issuers through replacing home-grown allowlist, denylist,
and jurisdiction checks baked into their own token contract with the
standardized, auditable contracts in this repo.

**Who this is for:** You already have a Stellar/Soroban token contract and
it contains hand-written compliance logic — e.g. a `mapping(address =>
bool) allowed` in a Soroban `Map`, or a function that rejects transfers to a
hardcoded list of blocked addresses.  You want to replace that bespoke code
with the independently reviewed primitives here.

**What you'll end up with:** One of two architectures (detailed below):

1. **Wrapper pattern** — deploy `allowlist-token` in front of your existing
   SEP-41 token; clients call the wrapper instead of the real token.
2. **Composition pattern** — wire `denylist-gate` and/or
   `jurisdiction-flag` into your own token's `transfer` via cross-contract
   calls, the same way `/examples/denylist-gate-consumer` does it.

Both are covered in this guide with concrete worked examples.

---

## Table of contents

- [Prerequisites](#prerequisites)
- [Step 1: Map your existing checks](#step-1-map-your-existing-checks)
  - [Allowlist → `allowlist-token`](#allowlist--allowlist-token)
  - [Denylist → `denylist-gate`](#denylist--denylist-gate)
  - [Jurisdiction check → `jurisdiction-flag`](#jurisdiction-check--jurisdiction-flag)
- [Step 2: Choose a migration pattern](#step-2-choose-a-migration-pattern)
- [Step 3: Deployment & wiring](#step-3-deployment--wiring)
  - [Pattern A: Wrapper (allowlist-token)](#pattern-a-wrapper-allowlist-token)
  - [Pattern B: Cross-contract composition](#pattern-b-cross-contract-composition)
- [Step 4: Data backfill](#step-4-data-backfill)
- [Step 5: Rollback plan](#step-5-rollback-plan)
- [Consolidating hand-wired composition into `policy-engine`](#consolidating-hand-wired-composition-into-policy-engine)
- [Worked example: `mapping(address => bool) allowed` → `allowlist-token`](#worked-example-mappingaddress--bool-allowed--allowlist-token)
- [Testing your migration](#testing-your-migration)

---

## Prerequisites

- `stellar` CLI installed (see the [Stellar CLI install docs](https://developers.stellar.org/docs/tools/developer-tools/cli))
- `wasm32v1-none` target: `rustup target add wasm32v1-none`
- A testnet identity configured: `stellar keys generate testnet-admin`
- This repo cloned and built:
  ```sh
  git clone https://github.com/stellar-compliance-kit/compliance-primitives.git
  cd compliance-primitives
  stellar contract build
  ```

---

## Step 1: Map your existing checks

Before deploying anything, identify which pattern(s) your current code uses.

### Allowlist → `allowlist-token`

**What you likely have today** — a storage entry that records which
addresses are KYC'd / onboarded, checked inside `transfer`:

```rust
// Typical hand-rolled allowlist (conceptual — your exact API will differ)
fn transfer(env: Env, from: Address, to: Address, amount: i128) {
    if !Self::is_allowed(env.clone(), from.clone())
        || !Self::is_allowed(env.clone(), to.clone())
    {
        panic!("address not allowlisted");
    }
    // … actual transfer logic …
}

fn is_allowed(env: Env, addr: Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Allowed(addr))
        .unwrap_or(false)
)
```

**The equivalent primitive:** [`allowlist-token`](./contracts/allowlist-token).
It wraps your underlying SEP-41 token and performs exactly this check in
its `transfer` before forwarding the call.  Your admin functions
(`add_to_allowlist`, `remove_from_allowlist`) map directly to the
corresponding functions on the wrapper.

| Your code | `allowlist-token` equivalent |
|---|---|
| `is_allowed(addr)` | `allowlist_token.is_allowed(&addr)` |
| `add_to_allowlist(admin, addr)` | `allowlist_token.add_to_allowlist(&admin, &addr, &None)` (pass `&Some(ledger)` to auto-expire) |
| `remove_from_allowlist(admin, addr)` | `allowlist_token.remove_from_allowlist(&admin, &addr)` |
| Inline allowlist check in `transfer` | Handled automatically by the wrapper |

### Denylist → `denylist-gate`

**What you likely have today** — a list of sanctioned/blocked addresses
checked on every transfer, often with a `require(!blocked)`-style guard:

```rust
// Typical hand-rolled denylist (conceptual)
fn transfer(env: Env, from: Address, to: Address, amount: i128) {
    if Self::is_blocked(env.clone(), from.clone())
        || Self::is_blocked(env.clone(), to.clone())
    {
        panic!("address is blocked");
    }
    // … actual transfer logic …
}
```

**The equivalent primitive:** [`denylist-gate`](./contracts/denylist-gate).
Deploy it standalone, then call `check(address)` via cross-contract call
before your token's `transfer` mutates any balances.  The gate returns
`true` when the address is *not* on the denylist (i.e. the address is
clear).

| Your code | `denylist-gate` equivalent |
|---|---|
| `is_blocked(addr)` / `blocked[addr]` | `!gate.check(&addr)` |
| `add_to_blocklist(admin, addr)` | `denylist_gate.add_to_denylist(&admin, &addr)` |
| `remove_from_blocklist(admin, addr)` | `denylist_gate.remove_from_denylist(&admin, &addr)` |

### Jurisdiction check → `jurisdiction-flag`

**What you likely have today** — a per-address country code and a permitted
list, checked inline:

```rust
// Typical hand-rolled jurisdiction check (conceptual)
fn transfer(env: Env, from: Address, to: Address, amount: i128) {
    let allowed = vec![&env, String::from_str(&env, "US"), String::from_str(&env, "CH")];
    if !Self::is_permitted(env.clone(), from.clone(), allowed.clone())
        || !Self::is_permitted(env.clone(), to.clone(), allowed)
    {
        panic!("address not in permitted jurisdiction");
    }
    // … actual transfer logic …
}
```

**The equivalent primitive:** [`jurisdiction-flag`](./contracts/jurisdiction-flag).
Deploy it standalone, set jurisdiction codes per address, then call
`is_permitted_jurisdiction(address, allowed_codes)` via cross-contract
call.

| Your code | `jurisdiction-flag` equivalent |
|---|---|
| `jurisdiction[addr] = "US"` | `jurisdiction_flag.set_jurisdiction(&issuer, &addr, &code)` |
| `get_jurisdiction(addr)` | `jurisdiction_flag.get_jurisdiction(&addr)` |
| Inline permitted-list check | `jurisdiction_flag.is_permitted_jurisdiction(&addr, &allowed_codes)` |

---

## Step 2: Choose a migration pattern

There are two fundamentally different approaches.  Pick the one that best
fits your architecture.

### Pattern A: Wrapper (for allowlist only)

Deploy `allowlist-token`, point it at your existing token contract, and
redirect clients to the wrapper's address.

- **Best when:** your token contract cannot be changed (already deployed,
  immutable, or you want zero code changes to the token itself).
- **Trade-off:** adds one extra hop per transfer (wrapper → underlying
  token).  The wrapper does not mint or burn; it only forwards.
- **The migration is a deployment + client redirect, not a code change.**

### Pattern B: Cross-contract composition (for denylist-gate and jurisdiction-flag)

Modify your token's `transfer` to call `check()` on a deployed
`denylist-gate` and/or `is_permitted_jurisdiction()` on
`jurisdiction-flag` before touching balances.  This is the pattern shown in
[`/examples/denylist-gate-consumer`](./examples/denylist-gate-consumer).

- **Best when:** you can deploy a new version of your token contract, or
  your token is still in development.
- **Trade-off:** requires a token redeploy, but gives you independent
  upgradeability — update the denylist without touching the token.

> **You can combine both patterns.** For instance, use `allowlist-token` as
> a wrapper for KYC gating *and* have the underlying token call
> `denylist-gate.check()` for sanctions screening.

---

## Step 3: Deployment & wiring

### Pattern A: Wrapper (allowlist-token)

```sh
# 1. Build the wrapper
stellar contract build

# 2. Deploy the allowlist-token contract
ALLOWLIST_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/allowlist_token.wasm \
  --source testnet-admin \
  --network testnet)

# 3. Initialize it, pointing at your real token
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source testnet-admin \
  --network testnet \
  -- initialize \
  --admin <your-admin-address> \
  --token <your-existing-token-contract-id>

# 4. Redirect all clients/dApps from your old token address to $ALLOWLIST_ID
echo "Wrapper deployed at $ALLOWLIST_ID"
```

At this point calls to `allowlist_token.transfer(from, to, amount)` (with
both parties allowlisted) will forward the transfer to your underlying
token.  Calls where either party is not allowlisted emit a `Blocked` event
and return `Ok(false)` without touching the underlying token.

### Pattern B: Cross-contract composition

This follows the pattern in
[`/examples/denylist-gate-consumer/src/lib.rs`](./examples/denylist-gate-consumer/src/lib.rs).

**1. Deploy the primitive(s):**

```sh
# Denylist gate
DENYLIST_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/denylist_gate.wasm \
  --source testnet-admin \
  --network testnet)

stellar contract invoke \
  --id "$DENYLIST_ID" \
  --source testnet-admin \
  --network testnet \
  -- initialize \
  --admin <your-admin-address>

# Jurisdiction flag
JURISDICTION_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/jurisdiction_flag.wasm \
  --source testnet-admin \
  --network testnet)

stellar contract invoke \
  --id "$JURISDICTION_ID" \
  --source testnet-admin \
  --network testnet \
  -- initialize \
  --issuer <your-issuer-address>
```

**2. Define a client trait in your token crate** (do NOT depend on the
primitive's crate directly — that would pull in duplicate WASM exports):

```rust
use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "JurisdictionClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}
```

**3. Store the gate/flag contract IDs in your token's instance storage**
(on initialize or via an admin setter):

```rust
fn initialize(env: Env, denylist_gate: Address, jurisdiction_flag: Address) {
    env.storage().instance().set(&DataKey::DenylistGate, &denylist_gate);
    env.storage().instance().set(&DataKey::JurisdictionFlag, &jurisdiction_flag);
}
```

**4. Add the cross-contract calls to your `transfer`** (BEFORE any balance
mutations):

```rust
fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
    from.require_auth();

    // --- compliance gates (add after require_auth, before balance ops) ---
    let gate_addr: Address = env.storage().instance()
        .get(&DataKey::DenylistGate)
        .ok_or(Error::NotInitialized)?;
    let gate = GateClient::new(&env, &gate_addr);
    if !gate.check(&from) || !gate.check(&to) {
        return Err(Error::DeniedByGate);
    }

    let jurisdiction_addr: Address = env.storage().instance()
        .get(&DataKey::JurisdictionFlag)
        .ok_or(Error::NotInitialized)?;
    let jurisdiction = JurisdictionClient::new(&env, &jurisdiction_addr);
    let allowed = Self::permitted_jurisdictions(env.clone());
    if !jurisdiction.is_permitted_jurisdiction(&from, &allowed)
        || !jurisdiction.is_permitted_jurisdiction(&to, &allowed)
    {
        return Err(Error::JurisdictionNotPermitted);
    }
    // --- end compliance gates ---

    // … existing balance checks & transfer logic …
}
```

---

## Step 4: Data backfill

After deploying the primitive contracts, you need to migrate the compliance
state from your old system.  The primitives' admin functions are designed
for batch invocation.

### Backfilling the allowlist

If your existing allowlist is stored in a database, spreadsheet, or
exportable from your old contract, iterate over it and call
`add_to_allowlist` for each address.  A typical script pattern:

```sh
#!/usr/bin/env bash
# Example: backfill allowlist from a CSV (one address per line)
while IFS= read -r addr; do
  stellar contract invoke \
    --id "$ALLOWLIST_ID" \
    --source testnet-admin \
    --network testnet \
    -- add_to_allowlist \
    --admin <your-admin-address> \
    --address "$addr"
done < allowlist.csv
```

> **Tip:** If you have thousands of entries, batch these invocations and
> submit them with fee bumps or through a multi-invoke helper to stay within
> network limits.

If your old allowlist was on-chain in a Soroban contract, you can write a
one-off migration contract that reads from the old contract and writes to
the new one in a single invocation per batch.

### Backfilling the denylist

Same pattern as the allowlist, using `denylist-gate`'s
`add_to_denylist(admin, address)`:

```sh
while IFS= read -r addr; do
  stellar contract invoke \
    --id "$DENYLIST_ID" \
    --source testnet-admin \
    --network testnet \
    -- add_to_denylist \
    --admin <your-admin-address> \
    --address "$addr"
done < denylist.csv
```

### Backfilling jurisdiction flags

For each address whose jurisdiction you already know, call
`set_jurisdiction`:

```sh
# CSV format: address,country_code
while IFS=, read -r addr code; do
  stellar contract invoke \
    --id "$JURISDICTION_ID" \
    --source testnet-admin \
    --network testnet \
    -- set_jurisdiction \
    --issuer <your-issuer-address> \
    --address "$addr" \
    --code "$code"
done < jurisdictions.csv
```

### Verifying the backfill

After backfill, spot-check a few known addresses:

```sh
# Check one address on the allowlist
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --network testnet \
  -- is_allowed --address <test-address>

# Check one address against the denylist (returns true = clear)
stellar contract invoke \
  --id "$DENYLIST_ID" \
  --network testnet \
  -- check --address <test-address>

# Check one address's jurisdiction
stellar contract invoke \
  --id "$JURISDICTION_ID" \
  --network testnet \
  -- get_jurisdiction --address <test-address>
```

---

## Step 5: Rollback plan

If something goes wrong post-migration (incorrect backfill, unexpected
behaviour, performance issues), you need a path back.

### Pattern A rollback (allowlist-token wrapper)

1. **Freeze the wrapper immediately** by removing all addresses from the
   allowlist — this makes every transfer return `Ok(false)` with a
   `Blocked` event, effectively pausing all movement without touching the
   underlying token:
   ```sh
   # Remove addresses from the allowlist (batch as needed)
   stellar contract invoke \
     --id "$ALLOWLIST_ID" --source testnet-admin --network testnet \
     -- remove_from_allowlist --admin <admin> --address <addr>
   ```
2. **Point clients back** to the original underlying token address.
3. **Investigate** the issue while the underlying token continues to
   operate normally (clients who haven't switched back yet will see their
   transfers blocked — that's intentional; it's a safe failure mode).

> The wrapper never holds funds — it only forwards, so there's nothing to
> "drain" or recover.  The underlying token is untouched throughout.

### Pattern B rollback (cross-contract composition)

1. **Deploy a hotfix version of your token** that either:
   - Removes the cross-contract calls (returning to the old inline checks),
     or
   - Points the gate/flag addresses to a known-good instance (e.g. an empty
     denylist if the gate was the problem).
2. **Or, deploy an empty denylist-gate** (no entries) as an emergency
   bypass — update your token's stored gate address to it:
   ```sh
   # Deploy a fresh (empty) denylist-gate as bypass
   BYPASS_ID=$(stellar contract deploy \
     --wasm target/wasm32v1-none/release/denylist_gate.wasm \
     --source testnet-admin --network testnet)
   stellar contract invoke --id "$BYPASS_ID" \
     --source testnet-admin --network testnet \
     -- initialize --admin <admin>
   # Update your token to point at the bypass gate
   stellar contract invoke --id "$YOUR_TOKEN_ID" \
     --source testnet-admin --network testnet \
     -- set_denylist_gate --gate "$BYPASS_ID"
   ```
   This lets transfers resume immediately while you debug the original
   gate.

---

## Consolidating hand-wired composition into `policy-engine`

**Who this section is for:** you already followed [Pattern B: Cross-contract
composition](#pattern-b-cross-contract-composition) above — your token's
`transfer` declares two `#[contractclient]` traits and calls
`denylist-gate` and `jurisdiction-flag` directly with two sequential `if`
checks — and you want the same AND-combined compliance decision with less
integration code sitting inside your token contract.
[`policy-engine`](../contracts/policy-engine) replaces both client traits
and both inline checks with a single cross-contract call; it doesn't
replace your `denylist-gate`/`jurisdiction-flag` deployments, it composes
them.

### Before: hand-wired composition

This is the code from [Step 3, Pattern
B](#pattern-b-cross-contract-composition) above:

```rust
#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "JurisdictionClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
    from.require_auth();

    let gate_addr: Address = env.storage().instance()
        .get(&DataKey::DenylistGate)
        .ok_or(Error::NotInitialized)?;
    let gate = GateClient::new(&env, &gate_addr);
    if !gate.check(&from) || !gate.check(&to) {
        return Err(Error::DeniedByGate);
    }

    let jurisdiction_addr: Address = env.storage().instance()
        .get(&DataKey::JurisdictionFlag)
        .ok_or(Error::NotInitialized)?;
    let jurisdiction = JurisdictionClient::new(&env, &jurisdiction_addr);
    let allowed = Self::permitted_jurisdictions(env.clone());
    if !jurisdiction.is_permitted_jurisdiction(&from, &allowed)
        || !jurisdiction.is_permitted_jurisdiction(&to, &allowed)
    {
        return Err(Error::JurisdictionNotPermitted);
    }

    // … existing balance checks & transfer logic …
}
```

Two stored addresses, two client traits, and both checks re-implemented by
hand for every consuming token.

### After: `policy-engine`

**1. Deploy `policy-engine` once.** It calls your existing `denylist-gate`
and `jurisdiction-flag` instances — you keep those deployments as-is:

```sh
POLICY_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/policy_engine.wasm \
  --source testnet-admin \
  --network testnet)

stellar contract invoke \
  --id "$POLICY_ID" \
  --source testnet-admin \
  --network testnet \
  -- initialize \
  --admin <your-admin-address> \
  --op All
```

`--op All` is the AND semantics your hand-wired `if !gate.check(...) ||
!jurisdiction...` code implemented manually — every registered check must
pass.

**2. Register the two checks you were previously calling by hand:**

```sh
stellar contract invoke --id "$POLICY_ID" --source testnet-admin --network testnet \
  -- add_check --admin <your-admin-address> \
  --check '{"Denylist":{"contract":"'"$DENYLIST_ID"'"}}'

stellar contract invoke --id "$POLICY_ID" --source testnet-admin --network testnet \
  -- add_check --admin <your-admin-address> \
  --check '{"Jurisdiction":{"contract":"'"$JURISDICTION_ID"'","allowed_codes":["US","CA","GB"]}}'
```

**3. Replace both client traits and both `if` checks with one call:**

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PolicyEngineError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    PolicyViolation = 4,
}

#[contractclient(name = "PolicyEngineClient")]
pub trait PolicyEngineInterface {
    fn evaluate(env: Env, from: Address, to: Address) -> Result<bool, PolicyEngineError>;
}

fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
    from.require_auth();

    let policy_addr: Address = env.storage().instance()
        .get(&DataKey::PolicyEngine)
        .ok_or(Error::NotInitialized)?;
    let policy = PolicyEngineClient::new(&env, &policy_addr);
    if !policy.evaluate(&from, &to) {
        return Err(Error::PolicyViolation);
    }

    // … existing balance checks & transfer logic …
}
```

`evaluate` runs every registered check against both `from` and `to` and
combines the results with the configured `CombineOp`, same as the hand-wired
version — but adding a third check (say, a second `denylist-gate` instance,
or swapping the combine operator to `Any`) is now an `add_check` /
re-`initialize` admin call against `policy-engine`, not a code change and
redeploy of your token.

**Trade-off to know before you migrate:** the hand-wired version's two
distinct errors (`DeniedByGate` vs. `JurisdictionNotPermitted`) become one
`PolicyViolation`, and `policy-engine`'s `PolicyResult` event carries
`passed`/`from`/`to` but not *which* registered check failed. If your
off-chain tooling depends on knowing which specific check rejected a
transfer, keep that in mind — everything downstream of `evaluate` sees a
single pass/fail decision.

**4. Update your rollback plan.** The [Pattern B rollback
steps](#pattern-b-rollback-cross-contract-composition) still apply, but
now point at `policy-engine`'s stored addresses: pointing your token at a
freshly-deployed, empty-checks `policy-engine` instance (`op: All`, no
`add_check` calls yet — `evaluate` returns `Ok(true)` with no checks
registered under `All`) is the equivalent emergency bypass to deploying an
empty `denylist-gate`.

---

## Worked example: `mapping(address => bool) allowed` → `allowlist-token`

> This is the concrete example promised in the acceptance criteria.

### The starting point

You have a token contract with a built-in allowlist pattern similar to:

```rust
// In your token contract today
fn transfer(env: Env, from: Address, to: Address, amount: i128) {
    if !Self::is_allowed(env.clone(), from.clone())
        || !Self::is_allowed(env.clone(), to.clone())
    {
        panic!("not allowlisted");
    }
    // … debit from, credit to, emit event …
}

fn is_allowed(env: Env, addr: Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Allowed(addr))
        .unwrap_or(false)
}

fn add_to_allowlist(env: Env, admin: Address, addr: Address) {
    admin.require_auth();
    env.storage().persistent().set(&DataKey::Allowed(addr), &true);
}
```

Your allowlist state lives at `DataKey::Allowed(addr)` in persistent
storage and you have ~500 addresses currently allowlisted.

### Step-by-step migration

**1. Build and deploy the primitive:**

```sh
stellar contract build

ALLOWLIST_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/allowlist_token.wasm \
  --source testnet-admin \
  --network testnet)
```

**2. Initialize with your underlying token:**

```sh
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source testnet-admin \
  --network testnet \
  -- initialize \
  --admin <your-admin-address> \
  --token <your-existing-token-contract-id>
```

**3. Backfill the allowlist.** Write a script that reads your old storage
and calls `add_to_allowlist` for each entry.  If your old state is in a
CSV exported from your backend:

```sh
#!/usr/bin/env bash
# Backfill 500 allowlist entries from CSV
ALLOWLIST_ID="<your-deployed-allowlist-id>"
ADMIN="<your-admin-address>"
while IFS= read -r addr; do
  stellar contract invoke \
    --id "$ALLOWLIST_ID" \
    --source testnet-admin \
    --network testnet \
    -- add_to_allowlist \
    --admin "$ADMIN" \
    --address "$addr"
done < allowlist.csv
```

If your old state is on-chain in a Soroban contract, write a one-shot Rust
contract that reads `DataKey::Allowed(addr)` from the old contract and
calls `add_to_allowlist` on the new one for each address found.

**4. Spot-check a few addresses:**

```sh
# Should return true for an address you backfilled
stellar contract invoke \
  --id "$ALLOWLIST_ID" --network testnet \
  -- is_allowed --address GABC...

# Should return false for a random address
stellar contract invoke \
  --id "$ALLOWLIST_ID" --network testnet \
  -- is_allowed --address GXYZ...
```

**5. Redirect clients.** Update your dApp, wallet integration, and any API
that points users at your token address to use `$ALLOWLIST_ID` instead.

**6. Monitor.** For the first 24 hours, watch the `Blocked` events from the
wrapper.  An unexpected spike in blocked transfers means your backfill
missed addresses that were allowlisted in the old system — re-run the
backfill for those addresses.

### What changed

| Before | After |
|---|---|
| Token contract handles both transfer logic AND allowlist checks | Token contract only handles transfer logic |
| Allowlist state lives in the token's storage | Allowlist state lives in a separate, auditable contract |
| Updating the allowlist means invoking the token contract | Updating the allowlist means invoking the wrapper |
| Changing allowlist logic requires token redeploy | Allowlist logic is decoupled; update the wrapper independently |

---

## Testing your migration

Before touching mainnet:

1. **Deploy everything to testnet** using the commands above (use
   `--network testnet`).
2. **Run the full test suite** for the primitives to make sure your
   environment is correct:
   ```sh
   cargo test --workspace
   ```
3. **Test a full end-to-end flow:**
   - Deploy a mock SEP-41 token to testnet
   - Deploy the primitive(s)
   - Initialize, backfill a few test addresses
   - Send a transfer from an allowlisted address to another allowlisted
     address — confirm it succeeds
   - Send a transfer involving a non-allowlisted address — confirm it is
     blocked and emits the expected event
   - Remove an address from the allowlist — confirm the next transfer for
     that address is blocked
4. **Test your rollback procedure** on testnet before you need it on
   mainnet.
