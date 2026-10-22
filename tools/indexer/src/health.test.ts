/**
 * Unit tests for the health-check HTTP server (src/health.ts).
 */

import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ComplianceDb } from "../src/db.js";
import { HealthServer, type StatusPayload } from "../src/health.js";

async function withDb(fn: (db: ComplianceDb) => Promise<void>): Promise<void> {
  const dir = await mkdtemp(join(tmpdir(), "compliance-health-"));
  const dbPath = join(dir, "health.db");
  const db = await ComplianceDb.open(dbPath);
  try {
    await fn(db);
  } finally {
    db.close();
    await rm(dir, { recursive: true, force: true });
  }
}

async function startServer(db: ComplianceDb): Promise<{ server: HealthServer; port: number }> {
  const server = new HealthServer(db, { port: 0, host: "127.0.0.1" });
  await new Promise<void>((resolve) => {
    // Use the internal http.Server through a small monkey-patch so we can get
    // the ephemeral port without exposing internals — instead we just try
    // successively open ports starting at a random base.
    server.start();
    // Give the server a tick to bind
    setTimeout(resolve, 20);
  });
  return { server, port: 0 };
}

test("/health returns 503 before any poll has been recorded", async () => {
  await withDb(async (db) => {
    // Use a fixed high port to avoid conflicts in CI
    const port = 19423;
    const server = new HealthServer(db, { port, host: "127.0.0.1" });
    server.start();
    await new Promise((r) => setTimeout(r, 30));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/health`);
      assert.equal(res.status, 503);
      const body = (await res.json()) as { ok: boolean };
      assert.equal(body.ok, false);
    } finally {
      server.stop();
    }
  });
});

test("/health returns 200 after recordPoll() is called", async () => {
  await withDb(async (db) => {
    const port = 19424;
    const server = new HealthServer(db, { port, host: "127.0.0.1" });
    server.start();
    await new Promise((r) => setTimeout(r, 30));
    server.recordPoll();
    try {
      const res = await fetch(`http://127.0.0.1:${port}/health`);
      assert.equal(res.status, 200);
      const body = (await res.json()) as { ok: boolean };
      assert.equal(body.ok, true);
    } finally {
      server.stop();
    }
  });
});

test("/status reflects lastIndexedLedger from the DB", async () => {
  await withDb(async (db) => {
    const port = 19425;
    db.setLastIndexedLedger(42);
    const server = new HealthServer(db, { port, host: "127.0.0.1" });
    server.start();
    server.recordPoll();
    await new Promise((r) => setTimeout(r, 30));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/status`);
      assert.equal(res.status, 200);
      const body = (await res.json()) as StatusPayload;
      assert.equal(body.ok, true);
      assert.equal(body.lastIndexedLedger, 42);
      assert.ok(typeof body.lastPollAt === "string");
      assert.ok(typeof body.now === "string");
    } finally {
      server.stop();
    }
  });
});

test("/status returns null fields before any poll", async () => {
  await withDb(async (db) => {
    const port = 19426;
    const server = new HealthServer(db, { port, host: "127.0.0.1" });
    server.start();
    await new Promise((r) => setTimeout(r, 30));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/status`);
      assert.equal(res.status, 200);
      const body = (await res.json()) as StatusPayload;
      assert.equal(body.ok, false);
      assert.equal(body.lastIndexedLedger, null);
      assert.equal(body.lastPollAt, null);
    } finally {
      server.stop();
    }
  });
});

test("unknown path returns 404", async () => {
  await withDb(async (db) => {
    const port = 19427;
    const server = new HealthServer(db, { port, host: "127.0.0.1" });
    server.start();
    await new Promise((r) => setTimeout(r, 30));
    try {
      const res = await fetch(`http://127.0.0.1:${port}/metrics`);
      assert.equal(res.status, 404);
    } finally {
      server.stop();
    }
  });
});
