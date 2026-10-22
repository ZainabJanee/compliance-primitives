/**
 * Core polling loop.
 *
 * On each tick:
 *  1. Call getEvents from (last_indexed_ledger + 1) to latestLedger
 *  2. Decode each event and apply it to the database
 *  3. Persist the new last_indexed_ledger
 *
 * The event endpoint's reported ledger horizon is checked against the last
 * checkpoint and the latest-ledger snapshot. Gaps and regressions are logged;
 * the indexer does not attempt to roll back a reorganization automatically.
 */

import type { Config } from "./config.js";
import type { ComplianceDb } from "./db.js";
import type { HealthServer } from "./health.js";
import { SorobanRpc } from "./rpc.js";
import { decodeEvent } from "./decoder.js";

export class Indexer {
  private rpc: SorobanRpc;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = false;

  constructor(
    private readonly config: Config,
    private readonly db: ComplianceDb,
    /** Optional health server; if provided, recordPoll() is called after each successful poll */
    private readonly health?: HealthServer
  ) {
    this.rpc = new SorobanRpc(config.rpcUrl);
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    console.log(`Indexer started. RPC: ${this.config.rpcUrl}`);
    console.log(`Poll interval: ${this.config.pollIntervalMs}ms`);
    void this.tick();
  }

  stop(): void {
    this.running = false;
    if (this.timer) clearTimeout(this.timer);
  }

  private schedule(): void {
    if (!this.running) return;
    this.timer = setTimeout(() => void this.tick(), this.config.pollIntervalMs);
  }

  private async tick(): Promise<void> {
    try {
      await this.poll();
      this.health?.recordPoll();
    } catch (err) {
      console.error("Poll error (will retry):", err);
    } finally {
      this.schedule();
    }
  }

  /** Run one poll cycle; useful for deterministic integration tests and operators. */
  async pollOnce(): Promise<void> {
    await this.poll();
  }

  private async poll(): Promise<void> {
    const contractIds = [
      this.config.allowlistContractId,
      this.config.denylistContractId,
      this.config.jurisdictionContractId,
      this.config.multisigContractId,
      this.config.aggregatorContractId,
      this.config.policyEngineContractId,
      this.config.circuitBreakerContractId,
    ].filter(Boolean);

    if (contractIds.length === 0) {
      console.warn("No contract IDs configured, skipping poll.");
      return;
    }

    // Determine start ledger
    const lastIndexed = this.db.getLastIndexedLedger();
    const startLedger =
      lastIndexed > 0
        ? lastIndexed + 1
        : this.config.startLedger > 0
        ? this.config.startLedger
        : await this.getEarliestAvailableLedger();

    if (lastIndexed > 0 && startLedger !== lastIndexed + 1) {
      const issue = startLedger < lastIndexed + 1 ? "regression" : "gap";
      console.warn(`Ledger ${issue} detected: poll starts at ${startLedger} after checkpoint ${lastIndexed}`);
    }

    const latest = await this.rpc.getLatestLedger();
    const targetLedger = this.config.endLedger > 0
      ? Math.min(latest, this.config.endLedger)
      : latest;

    if (this.config.endLedger > 0 && lastIndexed >= this.config.endLedger) {
      console.log(`Backfill complete through ledger ${this.config.endLedger}`);
      this.running = false;
      return;
    }

    if (startLedger > targetLedger) {
      // Already up to date
      return;
    }

    console.log(`Fetching events ledgers ${startLedger}–${targetLedger} (${contractIds.length} contract(s))`);

    // Paginate through all events in the range
    let cursor: string | undefined;
    let totalProcessed = 0;
    let eventHorizon: number | null = null;

    do {
      const result = await this.rpc.getEvents({
        startLedger,
        filters: [{ type: "contract", contractIds }],
        pagination: { limit: 200, cursor },
      });

      if (result.latestLedger < lastIndexed) {
        console.warn(`Ledger regression detected: getEvents reports ${result.latestLedger} after checkpoint ${lastIndexed}; retaining checkpoint`);
        return;
      }
      if (eventHorizon !== null && result.latestLedger < eventHorizon) {
        console.warn(`Ledger regression detected during pagination: ${result.latestLedger} after ${eventHorizon}; stopping at the last covered ledger`);
        break;
      }
      eventHorizon = result.latestLedger;

      const decoded = result.events
        .filter((event) => event.ledger >= startLedger && event.ledger <= targetLedger && event.ledger <= result.latestLedger)
        .map((e) => decodeEvent(e))
        .filter((e): e is NonNullable<typeof e> => e !== null);

      if (decoded.length > 0) {
        this.db.applyEvents(decoded);
        totalProcessed += decoded.length;
      }

      // Check if we got a full page (need to continue paginating)
      if (result.events.length === 200) {
        cursor = result.events[result.events.length - 1].pagingToken;
      } else {
        cursor = undefined;
      }

      if (this.config.endLedger > 0 && result.events.some((event) => event.ledger > targetLedger)) {
        break;
      }
    } while (cursor);

    if (eventHorizon === null || eventHorizon < targetLedger) {
      console.warn(`Ledger gap detected: requested through ${targetLedger}, but getEvents reports through ${eventHorizon ?? "no ledger"}`);
    }

    const indexedThrough = eventHorizon === null || eventHorizon < startLedger
      ? lastIndexed
      : Math.min(targetLedger, eventHorizon);
    if (indexedThrough > lastIndexed) this.db.setLastIndexedLedger(indexedThrough);

    if (totalProcessed > 0) {
      console.log(`Processed ${totalProcessed} event(s) through ledger ${indexedThrough}`);
    }

    if (this.config.endLedger > 0 && indexedThrough >= this.config.endLedger) {
      console.log(`Backfill complete through ledger ${this.config.endLedger}`);
      this.running = false;
    }
  }

  /**
   * Soroban RPC nodes only retain events for a finite window (~7 days on
   * testnet). If no startLedger is configured, we start from whatever the
   * node considers its oldest available ledger.
   *
   * We approximate this by fetching the latest ledger and subtracting the
   * typical retention window (17,280 ledgers ≈ 24h at 5s/ledger). A proper
   * implementation would call getLedgerEntries or use the node's
   * getLatestLedger response to discover the exact oldest available ledger.
   */
  private async getEarliestAvailableLedger(): Promise<number> {
    const latest = await this.rpc.getLatestLedger();
    // ~17280 ledgers ≈ 24 hours; use 1 as floor
    return Math.max(1, latest - 17280);
  }
}
