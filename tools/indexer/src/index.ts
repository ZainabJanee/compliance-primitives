/**
 * Entry point — routes CLI subcommands and wires config, DB, and Indexer
 * together for the poll loop.
 *
 * Subcommands:
 *   start   (default) — start the indexer poll loop
 *   query   — query indexed compliance events by address
 *
 * Usage:
 *   node dist/index.js start
 *   node dist/index.js query --address G...
 *   npm run start
 *   npm run query -- --address G...
 *
 * Pass `--dry-run` (or set `DRY_RUN=1`) to validate the configuration and
 * RPC connectivity without opening or writing to the database. Exits 0 on
 * success, 1 on any validation failure. Useful for CI pre-flight checks and
 * first-time operator setup.
 *
 * Optional health-check server
 * ────────────────────────────
 * Set HEALTH_PORT (e.g. HEALTH_PORT=8080) to start a minimal HTTP server
 * alongside the indexer. It exposes:
 *
 *   GET /health  — 200 OK after the first successful poll, 503 before that
 *   GET /status  — JSON with lastIndexedLedger and lastPollAt
 */

import process from "node:process";
import { loadConfig } from "./config.js";
import { ComplianceDb } from "./db.js";
import { HealthServer } from "./health.js";
import { Indexer } from "./indexer.js";
import { runQuery } from "./query.js";
import { SorobanRpc } from "./rpc.js";

async function dryRun(): Promise<void> {
  console.log("=== compliance-indexer dry-run ===\n");

  // 1. Validate config
  let config;
  try {
    config = loadConfig();
  } catch (err) {
    console.error("Config error:", (err as Error).message);
    process.exit(1);
  }

  console.log("Config:");
  console.log(`  RPC URL:          ${config.rpcUrl}`);
  console.log(`  Network:          ${config.networkPassphrase}`);
  console.log(`  DB path:          ${config.dbPath}`);
  console.log(`  Poll interval:    ${config.pollIntervalMs}ms`);
  console.log(`  Start ledger:     ${config.startLedger}`);
  const contracts: [string, string][] = [
    ["allowlist", config.allowlistContractId],
    ["denylist", config.denylistContractId],
    ["jurisdiction", config.jurisdictionContractId],
    ["multisig", config.multisigContractId],
    ["aggregator", config.aggregatorContractId],
    ["policy-engine", config.policyEngineContractId],
    ["circuit-breaker", config.circuitBreakerContractId],
  ];
  console.log("  Contracts:");
  for (const [name, id] of contracts) {
    console.log(`    ${name.padEnd(16)} ${id || "(not configured)"}`);
  }

  // 2. Test RPC connectivity
  console.log("\nChecking RPC connectivity…");
  try {
    const rpc = new SorobanRpc(config.rpcUrl);
    const ledger = await rpc.getLatestLedger();
    console.log(`  ✓ RPC reachable — latest ledger: ${ledger}`);
  } catch (err) {
    console.error("  ✗ RPC unreachable:", (err as Error).message);
    process.exit(1);
  }

  console.log("\nDry-run complete — config valid, RPC reachable.");
  process.exit(0);
}

async function main(): Promise<void> {
  const isDryRun =
    process.argv.includes("--dry-run") || process.env.DRY_RUN === "1";

  if (isDryRun) {
    await dryRun();
    return;
  }

  const config = loadConfig();
  const db = await ComplianceDb.open(config.dbPath);

  // Start the health/metrics HTTP server if HEALTH_PORT is configured.
  let health: HealthServer | undefined;
  const healthPortRaw = process.env.HEALTH_PORT?.trim();
  if (healthPortRaw) {
    const port = Number(healthPortRaw);
    if (!Number.isInteger(port) || port <= 0 || port > 65535) {
      throw new Error(`Invalid HEALTH_PORT: ${healthPortRaw} — must be a TCP port number (1–65535)`);
    }
    health = new HealthServer(db, { port });
    health.start();
  }

  const indexer = new Indexer(config, db, health);

  function shutdown(signal: string): void {
    console.log(`\nReceived ${signal}, shutting down…`);
    indexer.stop();
    health?.stop();
    db.close();
    process.exit(0);
  }

  process.on("SIGINT", () => shutdown("SIGINT"));
  process.on("SIGTERM", () => shutdown("SIGTERM"));

  indexer.start();
}

const subcommand = process.argv[2];

if (subcommand === "query") {
  // Pass remaining args after the subcommand name
  runQuery(process.argv.slice(3)).catch((err) => {
    console.error("Fatal:", err);
    process.exit(1);
  });
} else {
  // Default: start the indexer (also handles explicit "start", no subcommand,
  // or `--dry-run`)
  main().catch((err) => {
    console.error("Fatal:", err);
    process.exit(1);
  });
}
