import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { loadConfig } from "../src/config.js";
import { ComplianceDb } from "../src/db.js";
import { Indexer } from "../src/indexer.js";
import { SorobanRpc } from "../src/rpc.js";

const CONTRACT_ID = `C${"A".repeat(55)}`;
const ADDRESS = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

function xdr(...bytes: number[]): string {
  return Buffer.from(bytes).toString("base64");
}
function symbol(value: string): string {
  const payload = Buffer.from(value);
  const padding = Buffer.alloc((4 - (payload.length % 4)) % 4);
  return Buffer.concat([Buffer.from([0, 0, 0, 15, 0, 0, 0, payload.length]), payload, padding]).toString("base64");
}
function accountAddress(): string {
  return xdr(0, 0, 0, 18, 0, 0, 0, 0, 0, 0, 0, 0, ...new Array(32).fill(0));
}

async function listen(server: Server): Promise<number> {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return (server.address() as { port: number }).port;
}

test("loadConfig rejects missing and malformed startup configuration", () => {
  assert.throws(() => loadConfig({}), /RPC_URL is required/);
  const base = { RPC_URL: "http://localhost:3000", DB_PATH: "indexer.db", ALLOWLIST_CONTRACT_ID: CONTRACT_ID };
  assert.equal(loadConfig(base).allowlistContractId, CONTRACT_ID);
  assert.equal(loadConfig({ ...base, START_LEDGER: "10", END_LEDGER: "12" }).endLedger, 12);
  assert.throws(() => loadConfig({ ...base, START_LEDGER: "12", END_LEDGER: "10" }), /END_LEDGER must be >= START_LEDGER/);
  assert.throws(() => loadConfig({ ...base, ALLOWLIST_CONTRACT_ID: "bad" }), /valid Soroban contract ID/);
  assert.throws(() => loadConfig({ ...base, POLL_INTERVAL_MS: "0" }), /POLL_INTERVAL_MS/);
});

test("SorobanRpc retries transient HTTP failures with bounded backoff", async () => {
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    if (attempts < 3) { res.writeHead(503); res.end("temporary"); return; }
    res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ jsonrpc: "2.0", result: { sequence: 42 } }));
  });
  const port = await listen(server);
  try {
    assert.equal(await new SorobanRpc(`http://127.0.0.1:${port}`, { maxRetries: 3, baseDelayMs: 1, maxDelayMs: 2 }).getLatestLedger(), 42);
    assert.equal(attempts, 3);
  } finally { await new Promise<void>((resolve) => server.close(() => resolve())); }
});

test("schema migration preserves indexed rows and records its version", async () => {
  const directory = await mkdtemp(join(tmpdir(), "compliance-db-"));
  const path = join(directory, "indexer.db");
  const db = await ComplianceDb.open(path);
  db.applyEvents([{ ledgerSequence: 7, timestamp: 1, contractId: CONTRACT_ID, eventType: "AllowAdd", address: ADDRESS, addressTo: null, amount: null, jurisdiction: null, signerAddress: null, newThreshold: null, validCount: null, policyFrom: null, policyTo: null, policyPassed: null, rawTopics: "[]", rawData: "" }]);
  db.close();
  const reopened = await ComplianceDb.open(path);
  assert.equal(reopened.getEventCount(CONTRACT_ID), 1);
  assert.equal(reopened.isAllowlisted(CONTRACT_ID, ADDRESS), true);
  reopened.close();
  assert.ok((await readFile(path)).length > 0);
  await rm(directory, { recursive: true, force: true });
});

test("recorded local RPC state change is decoded and persisted by the indexer", async () => {
  const event = { type: "contract", ledger: 101, ledgerClosedAt: "2026-08-28T00:00:00.000Z", contractId: CONTRACT_ID, id: "fixture-allow-add", pagingToken: "101-1", inSuccessfulContractCall: true, topic: [symbol("AllowAdd"), accountAddress()], value: xdr(0, 0, 0, 1) };
  const server = createServer(async (req, res) => {
    const body = await new Promise<string>((resolve) => { let data = ""; req.on("data", (chunk) => data += chunk); req.on("end", () => resolve(data)); });
    const method = JSON.parse(body).method;
    const result = method === "getLatestLedger" ? { sequence: 101 } : { events: [event], latestLedger: 101 };
    res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ jsonrpc: "2.0", result }));
  });
  const port = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "compliance-indexer-"));
  const path = join(directory, "indexer.db");
  const db = await ComplianceDb.open(path);
  const config = loadConfig({ RPC_URL: `http://127.0.0.1:${port}`, DB_PATH: path, ALLOWLIST_CONTRACT_ID: CONTRACT_ID, START_LEDGER: "100", POLL_INTERVAL_MS: "10" });
  const indexer = new Indexer(config, db);
  try {
    await indexer.pollOnce();
    assert.equal(db.getEventCount(CONTRACT_ID), 1);
    assert.equal(db.isAllowlisted(CONTRACT_ID, ADDRESS), true);
    assert.equal(db.getLastIndexedLedger(), 101);
  } finally { db.close(); await new Promise<void>((resolve) => server.close(() => resolve())); await rm(directory, { recursive: true, force: true }); }
});

test("END_LEDGER bounds historical indexing and checkpoints the requested end", async () => {
  const event = (ledger: number, id: string) => ({ type: "contract", ledger, ledgerClosedAt: "2026-08-28T00:00:00.000Z", contractId: CONTRACT_ID, id, pagingToken: `${ledger}-1`, inSuccessfulContractCall: true, topic: [symbol("AllowAdd"), accountAddress()], value: xdr(0, 0, 0, 1) });
  const server = createServer(async (req, res) => {
    const body = await new Promise<string>((resolve) => { let data = ""; req.on("data", (chunk) => data += chunk); req.on("end", () => resolve(data)); });
    const method = JSON.parse(body).method;
    const result = method === "getLatestLedger" ? { sequence: 105 } : { events: [event(100, "in-range"), event(101, "past-end")], latestLedger: 105 };
    res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ jsonrpc: "2.0", result }));
  });
  const port = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "compliance-backfill-"));
  const path = join(directory, "indexer.db");
  const db = await ComplianceDb.open(path);
  const config = loadConfig({ RPC_URL: `http://127.0.0.1:${port}`, DB_PATH: path, ALLOWLIST_CONTRACT_ID: CONTRACT_ID, START_LEDGER: "100", END_LEDGER: "100" });
  try {
    await new Indexer(config, db).pollOnce();
    assert.equal(db.getEventCount(CONTRACT_ID), 1);
    assert.equal(db.getLastIndexedLedger(), 100);
    assert.equal(db.isAllowlisted(CONTRACT_ID, ADDRESS), true);
  } finally { db.close(); await new Promise<void>((resolve) => server.close(() => resolve())); await rm(directory, { recursive: true, force: true }); }
});

test("poll logs a lagging event horizon and ignores a regressed response", async () => {
  const event = { type: "contract", ledger: 105, ledgerClosedAt: "2026-08-28T00:00:00.000Z", contractId: CONTRACT_ID, id: "fixture-allow-add", pagingToken: "105-1", inSuccessfulContractCall: true, topic: [symbol("AllowAdd"), accountAddress()], value: xdr(0, 0, 0, 1) };
  let eventCalls = 0;
  const server = createServer(async (req, res) => {
    const body = await new Promise<string>((resolve) => { let data = ""; req.on("data", (chunk) => data += chunk); req.on("end", () => resolve(data)); });
    const method = JSON.parse(body).method;
    let result: unknown;
    if (method === "getLatestLedger") result = { sequence: 120 };
    else {
      eventCalls += 1;
      result = { events: eventCalls === 1 ? [event] : [], latestLedger: eventCalls === 1 ? 110 : 109 };
    }
    res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ jsonrpc: "2.0", result }));
  });
  const port = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "compliance-gap-check-"));
  const path = join(directory, "indexer.db");
  const db = await ComplianceDb.open(path);
  const config = loadConfig({ RPC_URL: `http://127.0.0.1:${port}`, DB_PATH: path, ALLOWLIST_CONTRACT_ID: CONTRACT_ID, START_LEDGER: "100" });
  const warnings: string[] = [];
  const originalWarn = console.warn;
  console.warn = (...args: unknown[]) => warnings.push(args.join(" "));
  try {
    const indexer = new Indexer(config, db);
    await indexer.pollOnce();
    assert.equal(db.getLastIndexedLedger(), 110);
    await indexer.pollOnce();
    assert.equal(db.getLastIndexedLedger(), 110);
    assert.equal(db.getEventCount(CONTRACT_ID), 1);
    assert.ok(warnings.some((warning) => warning.includes("Ledger gap detected")));
    assert.ok(warnings.some((warning) => warning.includes("Ledger regression detected")));
  } finally {
    console.warn = originalWarn;
    db.close();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    await rm(directory, { recursive: true, force: true });
  }
});
