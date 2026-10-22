/**
 * Minimal HTTP health-check / metrics server.
 *
 * Exposes two endpoints:
 *
 *   GET /health  — liveness probe; returns 200 OK once the indexer has
 *                  completed at least one poll, 503 Service Unavailable
 *                  before that.
 *
 *   GET /status  — machine-readable JSON document with the last indexed
 *                  ledger height and last-poll timestamp, suitable for a
 *                  process supervisor or monitoring system.
 *
 * Usage
 * ─────
 *   const health = new HealthServer(db, { port: 8080 });
 *   health.start();   // call after indexer.start()
 *   health.stop();    // call in the SIGINT/SIGTERM handler
 *
 * The server is entirely optional — if HEALTH_PORT is not set the process
 * behaves exactly as before.
 */

import { createServer, type Server } from "node:http";
import type { ComplianceDb } from "./db.js";

export interface HealthServerOptions {
  /** TCP port to listen on. Default: 8080 */
  port?: number;
  /** Host/interface to bind to. Default: "0.0.0.0" */
  host?: string;
}

export interface StatusPayload {
  /** Whether the indexer has successfully completed at least one poll */
  ok: boolean;
  /** Last indexed ledger sequence, or null if none yet */
  lastIndexedLedger: number | null;
  /** ISO-8601 timestamp of the last successful poll, or null if none yet */
  lastPollAt: string | null;
  /** ISO-8601 timestamp of when this response was generated */
  now: string;
}

export class HealthServer {
  private readonly server: Server;
  private lastPollAt: Date | null = null;

  constructor(
    private readonly db: ComplianceDb,
    private readonly options: HealthServerOptions = {}
  ) {
    this.server = createServer((req, res) => {
      const url = req.url?.split("?")[0] ?? "/";

      if (url === "/health") {
        const ready = this.lastPollAt !== null;
        res.setHeader("content-type", "application/json");
        res.writeHead(ready ? 200 : 503);
        res.end(JSON.stringify({ ok: ready }));
        return;
      }

      if (url === "/status") {
        const lastLedger = this.db.getLastIndexedLedger();
        const payload: StatusPayload = {
          ok: this.lastPollAt !== null,
          lastIndexedLedger: lastLedger > 0 ? lastLedger : null,
          lastPollAt: this.lastPollAt?.toISOString() ?? null,
          now: new Date().toISOString(),
        };
        res.setHeader("content-type", "application/json");
        res.writeHead(200);
        res.end(JSON.stringify(payload));
        return;
      }

      res.writeHead(404);
      res.end("Not found");
    });
  }

  /**
   * Record a successful poll tick. Call this after each poll cycle completes
   * without throwing so that /health reflects the indexer's live state.
   */
  recordPoll(): void {
    this.lastPollAt = new Date();
  }

  start(): void {
    const port = this.options.port ?? 8080;
    const host = this.options.host ?? "0.0.0.0";
    this.server.listen(port, host, () => {
      console.log(`Health server listening on http://${host}:${port} (/health, /status)`);
    });
  }

  stop(): void {
    this.server.close();
  }
}
