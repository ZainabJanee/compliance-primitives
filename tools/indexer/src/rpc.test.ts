/**
 * Tests for SorobanRpc retry / backoff behaviour (src/rpc.ts).
 *
 * Acceptance criteria from issue #426:
 *  ✓ Exponential backoff retry around getEvents for transient network/RPC errors
 *  ✓ Maximum retry count before surfacing the failure to the caller
 *  ✓ Test simulating a transient failure followed by success
 */

import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import test from "node:test";
import { SorobanRpc } from "../src/rpc.js";

// ── helpers ──────────────────────────────────────────────────────────────────

function listen(server: Server): Promise<number> {
  return new Promise<number>((resolve) =>
    server.listen(0, "127.0.0.1", () =>
      resolve((server.address() as { port: number }).port)
    )
  );
}

function close(server: Server): Promise<void> {
  return new Promise<void>((resolve) => server.close(() => resolve()));
}

const CONTRACT_ID = `C${"A".repeat(55)}`;

/** Minimal valid GetEvents JSON-RPC response */
function eventsResponse(events: unknown[] = []): string {
  return JSON.stringify({
    jsonrpc: "2.0",
    result: { events, latestLedger: 200 },
  });
}

// ── getEvents: transient failure then success ─────────────────────────────────

test("getEvents retries on a transient 503 and succeeds on the third attempt", async () => {
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    if (attempts < 3) {
      // 503 Service Unavailable — treated as transient
      res.writeHead(503);
      res.end("temporary overload");
      return;
    }
    res.setHeader("content-type", "application/json");
    res.end(eventsResponse());
  });
  const port = await listen(server);
  try {
    const rpc = new SorobanRpc(`http://127.0.0.1:${port}`, {
      maxRetries: 4,
      baseDelayMs: 1,
      maxDelayMs: 2,
    });
    const result = await rpc.getEvents({
      startLedger: 100,
      filters: [{ type: "contract", contractIds: [CONTRACT_ID] }],
    });
    assert.equal(result.latestLedger, 200);
    assert.deepEqual(result.events, []);
    // Two 503s then one success = 3 total HTTP requests
    assert.equal(attempts, 3);
  } finally {
    await close(server);
  }
});

test("getEvents retries on a transient JSON-RPC -32000 error", async () => {
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    res.setHeader("content-type", "application/json");
    if (attempts < 2) {
      // -32000 is treated as a transient RPC error
      res.end(
        JSON.stringify({
          jsonrpc: "2.0",
          error: { code: -32000, message: "node is still syncing" },
        })
      );
      return;
    }
    res.end(eventsResponse());
  });
  const port = await listen(server);
  try {
    const rpc = new SorobanRpc(`http://127.0.0.1:${port}`, {
      maxRetries: 3,
      baseDelayMs: 1,
      maxDelayMs: 2,
    });
    const result = await rpc.getEvents({
      startLedger: 1,
      filters: [{ type: "contract", contractIds: [CONTRACT_ID] }],
    });
    assert.equal(result.latestLedger, 200);
    assert.equal(attempts, 2);
  } finally {
    await close(server);
  }
});

test("getEvents surfaces the error after exhausting maxRetries", async () => {
  // Every request returns 503 — the RPC should give up after maxRetries
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    res.writeHead(503);
    res.end("always down");
  });
  const port = await listen(server);
  try {
    const rpc = new SorobanRpc(`http://127.0.0.1:${port}`, {
      maxRetries: 2,
      baseDelayMs: 1,
      maxDelayMs: 2,
    });
    await assert.rejects(
      () =>
        rpc.getEvents({
          startLedger: 1,
          filters: [{ type: "contract", contractIds: [CONTRACT_ID] }],
        }),
      /RPC HTTP error 503/
    );
    // 1 initial attempt + 2 retries = 3 total
    assert.equal(attempts, 3);
  } finally {
    await close(server);
  }
});

test("getEvents does not retry a non-transient 400 Bad Request", async () => {
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    res.writeHead(400);
    res.end("bad request");
  });
  const port = await listen(server);
  try {
    const rpc = new SorobanRpc(`http://127.0.0.1:${port}`, {
      maxRetries: 3,
      baseDelayMs: 1,
      maxDelayMs: 2,
    });
    await assert.rejects(
      () =>
        rpc.getEvents({
          startLedger: 1,
          filters: [{ type: "contract", contractIds: [CONTRACT_ID] }],
        }),
      /RPC HTTP error 400/
    );
    // Non-transient: should NOT be retried
    assert.equal(attempts, 1);
  } finally {
    await close(server);
  }
});

test("getLatestLedger retries transient failures with bounded backoff", async () => {
  let attempts = 0;
  const server = createServer((_req, res) => {
    attempts += 1;
    if (attempts < 3) {
      res.writeHead(503);
      res.end("temporary");
      return;
    }
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ jsonrpc: "2.0", result: { sequence: 42 } }));
  });
  const port = await listen(server);
  try {
    const rpc = new SorobanRpc(`http://127.0.0.1:${port}`, {
      maxRetries: 3,
      baseDelayMs: 1,
      maxDelayMs: 2,
    });
    assert.equal(await rpc.getLatestLedger(), 42);
    assert.equal(attempts, 3);
  } finally {
    await close(server);
  }
});
