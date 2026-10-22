# examples/js-client

A minimal TypeScript example showing how to call a deployed
`compliance-primitives` contract from off-chain code using
[`@stellar/stellar-sdk`](https://github.com/stellar/js-stellar-sdk).

Every other example in `/examples` is itself a Soroban contract (Rust). This
one is the **off-chain counterpart**: a Node.js script that invokes a deployed
`denylist-gate` contract's `check` function from JavaScript/TypeScript.

## What it demonstrates

- Connecting to a Soroban RPC endpoint (testnet by default)
- Building a read-only transaction that calls `denylist-gate.check(address)`
- Simulating the transaction — no signing or fees required for read-only calls
- Decoding the `bool` return value from XDR to a native JS value

## Prerequisites

- Node.js >= 20
- A deployed `denylist-gate` contract on testnet
  (see [`scripts/deploy-testnet.sh`](../../scripts/deploy-testnet.sh) or
  the root [QUICKSTART.md](../../QUICKSTART.md))

## Quick start

```sh
# 1. Install dependencies
cd examples/js-client
npm install

# 2. Configure
cp .env.example .env
# Edit .env: set DENYLIST_GATE_CONTRACT_ID and ADDRESS_TO_CHECK

# 3. Run
npm run check
```

Expected output (address not on denylist):

```
RPC endpoint : https://soroban-testnet.stellar.org
Contract     : CXXXXXXX...
Address      : GAAZI4T...

denylist-gate.check(GAAZI4T...) => ✅ allowed
```

If the address is on the denylist:

```
denylist-gate.check(GAAZI4T...) => 🚫 denied
```

## Extending this example

The same pattern works for any read-only function in any of the
compliance-primitives contracts. To call a different function:

1. Change `contract.call("check", ...)` to the function name and arguments
   you need.
2. Adjust the `scValToNative` decode to match the return type.

For `jurisdiction-flag.is_permitted_jurisdiction(address, allowed_codes)` you
would pass two arguments: an `address` ScVal and a `vec<string>` ScVal.
For `circuit-breaker.is_frozen()` you would pass no arguments at all.

## Relation to `tools/indexer`

[`tools/indexer`](../../tools/indexer) is a long-running event indexer that
uses the raw JSON-RPC interface directly. This example uses the higher-level
`@stellar/stellar-sdk` client, which is a better starting point when you
need to make individual contract calls rather than stream events.
