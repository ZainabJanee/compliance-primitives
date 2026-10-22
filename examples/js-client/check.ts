/**
 * Minimal TypeScript client example — calls denylist-gate.check(address)
 * against a deployed Soroban contract using @stellar/stellar-sdk.
 *
 * Usage:
 *   cp .env.example .env          # fill in contract ID and address
 *   npm install
 *   npm run check
 *
 * Or pass values via environment variables directly:
 *   DENYLIST_GATE_CONTRACT_ID=C... ADDRESS_TO_CHECK=G... npm run check
 */

import {
  Contract,
  Networks,
  nativeToScVal,
  scValToNative,
  SorobanRpc,
  TransactionBuilder,
  Account,
  BASE_FEE,
  Address,
} from "@stellar/stellar-sdk";
import { readFileSync, existsSync } from "node:fs";

// ---------------------------------------------------------------------------
// Load environment (from .env file if present, else process.env)
// ---------------------------------------------------------------------------

function loadEnv(): void {
  const envPath = new URL(".env", import.meta.url).pathname;
  if (!existsSync(envPath)) return;
  const lines = readFileSync(envPath, "utf-8").split("\n");
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq === -1) continue;
    const key = trimmed.slice(0, eq).trim();
    const val = trimmed.slice(eq + 1).trim().replace(/^"|"$/g, "");
    if (!(key in process.env)) process.env[key] = val;
  }
}

loadEnv();

const CONTRACT_ID = process.env.DENYLIST_GATE_CONTRACT_ID ?? "";
const ADDRESS_TO_CHECK = process.env.ADDRESS_TO_CHECK ?? "";
const RPC_URL =
  process.env.STELLAR_RPC_URL ?? "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE =
  process.env.NETWORK_PASSPHRASE ?? Networks.TESTNET;

if (!CONTRACT_ID || !ADDRESS_TO_CHECK) {
  console.error(
    "Error: DENYLIST_GATE_CONTRACT_ID and ADDRESS_TO_CHECK must be set.\n" +
      "Copy .env.example to .env and fill in the values, or set them as\n" +
      "environment variables before running this script."
  );
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Call denylist-gate.check(address) — a read-only simulation
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  const server = new SorobanRpc.Server(RPC_URL);

  // For a read-only simulation we don't need a funded account. We use the
  // contract address itself as a dummy source — the simulation engine only
  // requires a structurally valid transaction, not a funded one.
  const source = new Account(ADDRESS_TO_CHECK, "0");

  const contract = new Contract(CONTRACT_ID);

  // Build the transaction invoking check(address_to_check).
  // `check` takes a single Soroban Address argument.
  const tx = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(
      contract.call(
        "check",
        nativeToScVal(Address.fromString(ADDRESS_TO_CHECK), { type: "address" })
      )
    )
    .setTimeout(30)
    .build();

  console.log(`RPC endpoint : ${RPC_URL}`);
  console.log(`Contract     : ${CONTRACT_ID}`);
  console.log(`Address      : ${ADDRESS_TO_CHECK}`);
  console.log();

  // Simulate — no signing or submission needed for a read-only call.
  const result = await server.simulateTransaction(tx);

  if (SorobanRpc.Api.isSimulationError(result)) {
    console.error("Simulation failed:", result.error);
    process.exit(1);
  }

  if (!SorobanRpc.Api.isSimulationSuccess(result)) {
    console.error("Unexpected simulation response:", result);
    process.exit(1);
  }

  // The return value is a ScVal (XDR). Convert it to a JS native value.
  const returnVal = result.result?.retval;
  if (returnVal === undefined) {
    console.error("No return value in simulation result");
    process.exit(1);
  }

  const isAllowed: boolean = scValToNative(returnVal) as boolean;

  console.log(
    `denylist-gate.check(${ADDRESS_TO_CHECK}) => ${isAllowed ? "✅ allowed" : "🚫 denied"}`
  );
}

main().catch((err) => {
  console.error("Unexpected error:", err);
  process.exit(1);
});
