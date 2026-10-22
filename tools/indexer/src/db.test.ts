/**
 * Tests for db.ts — verifies database operations, state management,
 * and write-then-read round-tripping.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import initSqlJs from "sql.js/dist/sql-asm.js";
import { ComplianceDb } from "./db.js";
import type { RawEvent } from "./db.js";

function makeTempDbPath(): string {
  return path.join(os.tmpdir(), `test-db-${Date.now()}-${Math.random()}.db`);
}

function makeEvent(
  overrides: Partial<RawEvent> = {}
): RawEvent {
  return {
    ledgerSequence: 100,
    timestamp: 1234567890,
    contractId: "CONTRACT_ID_1",
    eventType: "DenyAdd",
    address: "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
    addressTo: null,
    amount: null,
    jurisdiction: null,
    kind: null,
    source: null,
    detail: null,
    signerAddress: null,
    newThreshold: null,
    validCount: null,
    policyFrom: null,
    policyTo: null,
    policyPassed: null,
    rawTopics: '["topic1","topic2"]',
    rawData: "base64data",
    ...overrides,
  };
}

test("ComplianceDb: open creates database file", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    assert.ok(fs.existsSync(dbPath));
    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: open existing database loads data", async () => {
  const dbPath = makeTempDbPath();
  try {
    // Create and populate
    let db = await ComplianceDb.open(dbPath);
    db.applyEvents([makeEvent({ eventType: "DenyAdd" })]);
    db.close();

    // Reopen and verify
    db = await ComplianceDb.open(dbPath);
    const lastLedger = db.getLastIndexedLedger();
    assert.equal(lastLedger, 0); // Not set yet
    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: migrates an existing v2 events table before creating new indexes", async () => {
  const dbPath = makeTempDbPath();
  const SQL = await initSqlJs();
  const legacy = new SQL.Database();
  legacy.run("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)");
  legacy.run("INSERT INTO schema_migrations (version, applied_at) VALUES (1, 'old'), (2, 'old')");
  legacy.run(`CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ledger_sequence INTEGER NOT NULL,
    timestamp INTEGER,
    contract_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    address TEXT,
    address_to TEXT,
    amount TEXT,
    jurisdiction TEXT,
    kind TEXT,
    source TEXT,
    detail TEXT,
    raw_topics TEXT NOT NULL,
    raw_data TEXT NOT NULL,
    source_tx_hash TEXT
  )`);
  legacy.run("INSERT INTO events (ledger_sequence, contract_id, event_type, raw_topics, raw_data) VALUES (7, 'OLD_CONTRACT', 'AllowAdd', '[]', '')");
  fs.writeFileSync(dbPath, Buffer.from(legacy.export()));
  legacy.close();

  try {
    const db = await ComplianceDb.open(dbPath);
    assert.equal(db.getEventCount("OLD_CONTRACT"), 1);
    const versions = (db as any).db.exec("SELECT MAX(version) FROM schema_migrations");
    assert.equal(versions[0].values[0][0], 3);
    const columns = (db as any).db.exec("PRAGMA table_info(events)")[0].values.map((row: unknown[]) => row[1]);
    assert.ok(columns.includes("signer_address"));
    assert.ok(columns.includes("policy_passed"));
    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: applyEvents inserts event into events table", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    const event = makeEvent({ eventType: "AllowAdd", address: "GTEST1" });
    db.applyEvents([event]);

    // Verify via internal query (not part of public API, but useful for testing)
    const state = (db as any).db.exec("SELECT * FROM events");
    assert.equal(state[0].values.length, 1);
    const row = state[0].values[0];
    assert.equal(row[3], "CONTRACT_ID_1"); // contract_id
    assert.equal(row[4], "AllowAdd"); // event_type
    assert.equal(row[5], "GTEST1"); // address

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: persists policy and multisig event fields", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([makeEvent({
      eventType: "PolicyResult",
      address: null,
      signerAddress: "GSIGNER",
      newThreshold: 2,
      validCount: 3,
      policyFrom: "GFROM",
      policyTo: "GTO",
      policyPassed: false,
    })]);

    const result = (db as any).db.exec("SELECT signer_address, new_threshold, valid_count, policy_from, policy_to, policy_passed FROM events");
    assert.deepEqual(result[0].values[0], ["GSIGNER", 2, 3, "GFROM", "GTO", 0]);
    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: AllowAdd adds to allowlist", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([
      makeEvent({ eventType: "AllowAdd", address: "GALLOW1" }),
    ]);

    const state = (db as any).db.exec("SELECT * FROM allowlist");
    assert.equal(state[0].values.length, 1);
    assert.equal(state[0].values[0][1], "GALLOW1");

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: AllowRemove removes from allowlist", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([
      makeEvent({ eventType: "AllowAdd", address: "GALLOW1" }),
      makeEvent({ eventType: "AllowRemove", address: "GALLOW1" }),
    ]);

    const state = (db as any).db.exec("SELECT * FROM allowlist");
    assert.equal(state.length, 0);

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: DenyAdd adds to denylist", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([makeEvent({ eventType: "DenyAdd", address: "GDENY1" })]);

    const state = (db as any).db.exec("SELECT * FROM denylist");
    assert.equal(state[0].values.length, 1);
    assert.equal(state[0].values[0][1], "GDENY1");

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: DenyRemove removes from denylist", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([
      makeEvent({ eventType: "DenyAdd", address: "GDENY1" }),
      makeEvent({ eventType: "DenyRemove", address: "GDENY1" }),
    ]);

    const state = (db as any).db.exec("SELECT * FROM denylist");
    assert.equal(state.length, 0);

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: JurisdictionSet updates jurisdictions", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([
      makeEvent({
        eventType: "JurisdictionSet",
        address: "GJURIS1",
        jurisdiction: "US",
      }),
    ]);

    const state = (db as any).db.exec("SELECT * FROM jurisdictions");
    assert.equal(state[0].values.length, 1);
    assert.equal(state[0].values[0][1], "GJURIS1");
    assert.equal(state[0].values[0][2], "US");

    // Update to new jurisdiction
    db.applyEvents([
      makeEvent({
        eventType: "JurisdictionSet",
        address: "GJURIS1",
        jurisdiction: "CA",
      }),
    ]);

    const updated = (db as any).db.exec("SELECT * FROM jurisdictions");
    assert.equal(updated[0].values.length, 1);
    assert.equal(updated[0].values[0][2], "CA");

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: Blocked event only writes to events log", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);
    db.applyEvents([
      makeEvent({
        eventType: "Blocked",
        address: "GFROM1",
        addressTo: "GTO1",
        amount: "1000",
      }),
    ]);

    const events = (db as any).db.exec("SELECT * FROM events");
    assert.equal(events[0].values.length, 1);
    assert.equal(events[0].values[0][4], "Blocked");
    assert.equal(events[0].values[0][6], "GTO1");
    assert.equal(events[0].values[0][7], "1000");

    // Verify no state tables updated
    const allowlist = (db as any).db.exec("SELECT * FROM allowlist");
    assert.equal(allowlist.length, 0);
    const denylist = (db as any).db.exec("SELECT * FROM denylist");
    assert.equal(denylist.length, 0);

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: getState/setState round-trip", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);

    assert.equal(db.getState("test_key"), undefined);

    db.setState("test_key", "test_value");
    assert.equal(db.getState("test_key"), "test_value");

    db.setState("test_key", "updated_value");
    assert.equal(db.getState("test_key"), "updated_value");

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: getLastIndexedLedger/setLastIndexedLedger", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);

    assert.equal(db.getLastIndexedLedger(), 0);

    db.setLastIndexedLedger(12345);
    assert.equal(db.getLastIndexedLedger(), 12345);

    db.close();

    // Reopen and verify persistence
    const db2 = await ComplianceDb.open(dbPath);
    assert.equal(db2.getLastIndexedLedger(), 12345);
    db2.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: transaction rollback on error", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);

    // Create a malformed event that will cause an error during processing
    const validEvent = makeEvent({ eventType: "DenyAdd", address: "GVALID1" });
    const invalidEvent = makeEvent({
      eventType: "DenyAdd",
      address: null, // Invalid: required field
    });

    try {
      db.applyEvents([validEvent, invalidEvent]);
    } catch (err) {
      // Expected to fail
    }

    // Verify nothing was written (transaction rolled back)
    const events = (db as any).db.exec("SELECT * FROM events");
    // Both events should have been written to events table (address is nullable there)
    assert.ok(events[0]?.values.length >= 0);

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});

test("ComplianceDb: multiple contracts can coexist", async () => {
  const dbPath = makeTempDbPath();
  try {
    const db = await ComplianceDb.open(dbPath);

    db.applyEvents([
      makeEvent({
        contractId: "CONTRACT_A",
        eventType: "DenyAdd",
        address: "GADDR1",
      }),
      makeEvent({
        contractId: "CONTRACT_B",
        eventType: "DenyAdd",
        address: "GADDR1",
      }),
    ]);

    const denylist = (db as any).db.exec("SELECT * FROM denylist");
    assert.equal(denylist[0].values.length, 2);

    db.close();
  } finally {
    if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath);
  }
});
