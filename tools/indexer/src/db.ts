/**
 * Database layer — SQLite via sql.js (pure WebAssembly, no native build).
 *
 * sql.js operates in-memory and does not auto-persist to disk; we flush the
 * database file after every write batch so restarts pick up where they left
 * off.
 *
 * Schema
 * ──────
 *
 * events
 *   id              INTEGER PK AUTOINCREMENT
 *   ledger_sequence INTEGER NOT NULL
 *   timestamp       INTEGER            — Unix seconds (ledger close time)
 *   contract_id     TEXT NOT NULL
 *   event_type      TEXT NOT NULL      — AllowAdd | AllowRemove | Blocked |
 *                                        DenyAdd | DenyRemove | JurisdictionSet |
 *                                        ComplianceEvent | SignerAdd | SignerRm |
 *                                        ThreshSet | AuthOk | PolicyResult |
 *                                        AdminSet | DenylistGateSet |
 *                                        JurisdictionFlagSet | Frozen | Unfrozen
 *   address         TEXT               — primary subject address
 *   address_to      TEXT               — secondary address (Blocked only)
 *   amount          TEXT               — i128 as decimal string (Blocked only)
 *   jurisdiction    TEXT               — ISO code (JurisdictionSet only)
 *   kind            TEXT               — audit-log event kind (ComplianceEvent only)
 *   source          TEXT               — audit-log source address (ComplianceEvent only)
 *   detail          TEXT               — audit-log detail string (ComplianceEvent only)
 *   signer_address  TEXT               — multisig-admin signer (SignerAdd/SignerRm only)
 *   new_threshold   INTEGER            — multisig-admin threshold (ThreshSet/AuthOk only)
 *   valid_count     INTEGER            — multisig-admin valid count (AuthOk only)
 *   policy_from     TEXT               — policy-engine from address (PolicyResult only)
 *   policy_to       TEXT               — policy-engine to address (PolicyResult only)
 *   policy_passed   INTEGER            — policy-engine result 0/1 (PolicyResult only)
 *   raw_topics      TEXT NOT NULL      — JSON array of base64-XDR topic strings
 *   raw_data        TEXT NOT NULL      — base64-XDR data value
 *
 * allowlist          — materialised current state
 *   contract_id TEXT NOT NULL
 *   address     TEXT NOT NULL
 *   PRIMARY KEY (contract_id, address)
 *
 * denylist           — materialised current state
 *   contract_id TEXT NOT NULL
 *   address     TEXT NOT NULL
 *   PRIMARY KEY (contract_id, address)
 *
 * jurisdictions      — materialised current state (last write wins)
 *   contract_id TEXT NOT NULL
 *   address     TEXT NOT NULL
 *   code        TEXT NOT NULL
 *   PRIMARY KEY (contract_id, address)
 *
 * multisig_signers     — current signer set per multisig-admin contract
 *   contract_id TEXT NOT NULL
 *   address     TEXT NOT NULL
 *   PRIMARY KEY (contract_id, address)
 *
 * multisig_threshold   — current threshold per multisig-admin contract
 *   contract_id TEXT PK
 *   threshold   INTEGER NOT NULL
 *
 * aggregator_config  — current gate/flag addresses per aggregator contract
 *   contract_id      TEXT NOT NULL     — the aggregator contract
 *   config_key       TEXT NOT NULL     — "admin" | "denylist_gate" | "jurisdiction_flag"
 *   config_address   TEXT NOT NULL     — the currently configured address
 *   PRIMARY KEY (contract_id, config_key)
 *
 * circuit_breaker_state — current frozen/unfrozen state per circuit-breaker
 *   contract_id TEXT PRIMARY KEY
 *   is_frozen   INTEGER NOT NULL       — 1 = frozen, 0 = unfrozen
 *
 * indexer_state      — internal key/value (stores last_ledger)
 *   key   TEXT PK
 *   value TEXT NOT NULL
 */

import fs from "node:fs";
import initSqlJs from "sql.js/dist/sql-asm.js";
import type { Database, SqlJsStatic } from "sql.js";

export interface RawEvent {
  ledgerSequence: number;
  timestamp: number | null;
  contractId: string;
  eventType: string;
  address: string | null;
  addressTo: string | null;
  amount: string | null;
  jurisdiction: string | null;
  /** Populated for ComplianceEvent: the kind symbol value (e.g. "deny_add") */
  kind: string | null;
  /** Populated for ComplianceEvent: the source address that called record() */
  source: string | null;
  /** Populated for ComplianceEvent: the free-form detail string */
  detail: string | null;
  /**
   * Populated for PolicyResult events: the `from` address evaluated by
   * the policy engine (topics: [Symbol("PolicyResult"), Bool(passed)],
   * data: Vec[Address(from), Address(to)]).
   */
  policyFrom: string | null;
  /**
   * Populated for PolicyResult events: the `to` address evaluated by
   * the policy engine.
   */
  policyTo: string | null;
  /**
   * Populated for PolicyResult events: whether the policy evaluation
   * passed (true) or failed (false).
   */
  policyPassed: boolean | null;
  /**
   * Populated for multisig-admin SignerAdded/SignerRemoved events:
   * the signer address that was added or removed.
   * topics: [Symbol("SignerAdd"|"SignerRm"), Address(signer)], data: Void
   */
  signerAddress: string | null;
  /**
   * Populated for multisig-admin ThresholdUpdated and AuthOk events:
   * the (new) threshold value.
   * ThreshSet: data U32(threshold)
   * AuthOk:    data Vec[U32(valid_count), U32(threshold)]
   */
  newThreshold: number | null;
  /**
   * Populated for multisig-admin AuthOk events:
   * the number of valid signatures that satisfied the threshold.
   * topics: [Symbol("AuthOk")], data: Vec[U32(valid_count), U32(threshold)]
   */
  validCount: number | null;
  rawTopics: string;
  rawData: string;
}

// sql.js is loaded once as a module-level singleton
let SQL: SqlJsStatic | null = null;
async function getSql(): Promise<SqlJsStatic> {
  if (!SQL) {
    SQL = await initSqlJs({
      locateFile: (file: string) => new URL(`../node_modules/sql.js/dist/${file}`, import.meta.url).pathname,
    });
  }
  return SQL;
}

export class ComplianceDb {
  private db!: Database;
  private dbPath: string;

  private constructor(dbPath: string) {
    this.dbPath = dbPath;
  }

  static async open(dbPath: string): Promise<ComplianceDb> {
    const sql = await getSql();
    const inst = new ComplianceDb(dbPath);

    if (fs.existsSync(dbPath)) {
      const data = fs.readFileSync(dbPath);
      inst.db = new sql.Database(data);
    } else {
      inst.db = new sql.Database();
    }

    inst.migrate();
    return inst;
  }

  private migrate(): void {
    this.db.run(`CREATE TABLE IF NOT EXISTS schema_migrations (
      version INTEGER PRIMARY KEY,
      applied_at TEXT NOT NULL
    );`);
    const currentVersion = this.db.exec("SELECT COALESCE(MAX(version), 0) AS version FROM schema_migrations")[0]?.values[0]?.[0] as number ?? 0;
    this.db.run(`
      CREATE TABLE IF NOT EXISTS events (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        ledger_sequence INTEGER NOT NULL,
        timestamp       INTEGER,
        contract_id     TEXT    NOT NULL,
        event_type      TEXT    NOT NULL,
        address         TEXT,
        address_to      TEXT,
        amount          TEXT,
        jurisdiction    TEXT,
        kind            TEXT,
        source          TEXT,
        detail          TEXT,
        signer_address  TEXT,
        new_threshold   INTEGER,
        valid_count     INTEGER,
        policy_from     TEXT,
        policy_to       TEXT,
        policy_passed   INTEGER,
        raw_topics      TEXT    NOT NULL,
        raw_data        TEXT    NOT NULL
      );

      CREATE INDEX IF NOT EXISTS idx_events_contract
        ON events (contract_id);
      CREATE INDEX IF NOT EXISTS idx_events_address
        ON events (address);
      CREATE INDEX IF NOT EXISTS idx_events_type
        ON events (event_type);
      CREATE INDEX IF NOT EXISTS idx_events_ledger
        ON events (ledger_sequence);
      CREATE TABLE IF NOT EXISTS allowlist (
        contract_id TEXT NOT NULL,
        address     TEXT NOT NULL,
        PRIMARY KEY (contract_id, address)
      );

      CREATE TABLE IF NOT EXISTS denylist (
        contract_id TEXT NOT NULL,
        address     TEXT NOT NULL,
        PRIMARY KEY (contract_id, address)
      );

      CREATE TABLE IF NOT EXISTS jurisdictions (
        contract_id TEXT NOT NULL,
        address     TEXT NOT NULL,
        code        TEXT NOT NULL,
        PRIMARY KEY (contract_id, address)
      );

      CREATE TABLE IF NOT EXISTS audit_log (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        contract_id TEXT    NOT NULL,
        ledger      INTEGER NOT NULL,
        timestamp   INTEGER,
        kind        TEXT    NOT NULL,
        subject     TEXT    NOT NULL,
        source      TEXT    NOT NULL,
        detail      TEXT    NOT NULL
      );

      CREATE INDEX IF NOT EXISTS idx_audit_log_contract
        ON audit_log (contract_id);
      CREATE INDEX IF NOT EXISTS idx_audit_log_subject
        ON audit_log (subject);
      CREATE INDEX IF NOT EXISTS idx_audit_log_kind
        ON audit_log (kind);

      CREATE TABLE IF NOT EXISTS indexer_state (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );
    `);
    if (currentVersion < 1) {
      this.db.run("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)", [1, new Date().toISOString()]);
    }
    if (currentVersion < 2) {
      const columns = this.db.exec("PRAGMA table_info(events)")[0]?.values.map((row) => String(row[1])) ?? [];
      if (!columns.includes("source_tx_hash")) this.db.run("ALTER TABLE events ADD COLUMN source_tx_hash TEXT");
      this.db.run("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)", [2, new Date().toISOString()]);
    }
    if (currentVersion < 3) {
      const columns = this.db.exec("PRAGMA table_info(events)")[0]?.values.map((row) => String(row[1])) ?? [];
      if (!columns.includes("signer_address")) this.db.run("ALTER TABLE events ADD COLUMN signer_address TEXT");
      if (!columns.includes("new_threshold"))  this.db.run("ALTER TABLE events ADD COLUMN new_threshold INTEGER");
      if (!columns.includes("valid_count"))    this.db.run("ALTER TABLE events ADD COLUMN valid_count INTEGER");
      if (!columns.includes("policy_from"))    this.db.run("ALTER TABLE events ADD COLUMN policy_from TEXT");
      if (!columns.includes("policy_to"))      this.db.run("ALTER TABLE events ADD COLUMN policy_to TEXT");
      if (!columns.includes("policy_passed"))  this.db.run("ALTER TABLE events ADD COLUMN policy_passed INTEGER");
      this.db.run("INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)", [3, new Date().toISOString()]);
    }
    this.db.run("CREATE INDEX IF NOT EXISTS idx_events_signer ON events (signer_address)");
    this.db.run("CREATE INDEX IF NOT EXISTS idx_events_policy_from ON events (policy_from)");
    this.db.run("CREATE INDEX IF NOT EXISTS idx_events_policy_to ON events (policy_to)");
    this.flush();
  }

  /** Flush the in-memory database to disk. */
  private flush(): void {
    const data = this.db.export();
    fs.writeFileSync(this.dbPath, Buffer.from(data));
  }

  /** Apply a batch of events atomically, then flush to disk. */
  applyEvents(events: RawEvent[]): void {
    this.db.run("BEGIN");
    try {
      for (const e of events) {
        this.insertEvent(e);
        this.updateState(e);
      }
      this.db.run("COMMIT");
    } catch (err) {
      this.db.run("ROLLBACK");
      throw err;
    }
    this.flush();
  }

  private insertEvent(e: RawEvent): void {
    this.db.run(
      `INSERT INTO events
         (ledger_sequence, timestamp, contract_id, event_type,
          address, address_to, amount, jurisdiction,
         kind, source, detail, signer_address, new_threshold, valid_count,
         policy_from, policy_to, policy_passed, raw_topics, raw_data)
       VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
      [
        e.ledgerSequence,
        e.timestamp,
        e.contractId,
        e.eventType,
        e.address ?? null,
        e.addressTo ?? null,
        e.amount ?? null,
        e.jurisdiction ?? null,
        e.kind ?? null,
        e.source ?? null,
        e.detail ?? null,
        e.signerAddress ?? null,
        e.newThreshold ?? null,
        e.validCount ?? null,
        e.policyFrom ?? null,
        e.policyTo ?? null,
        e.policyPassed == null ? null : Number(e.policyPassed),
        e.rawTopics,
        e.rawData,
      ]
    );
  }

  private updateState(e: RawEvent): void {
    switch (e.eventType) {
      case "AllowAdd":
        if (e.address) {
          this.db.run(
            "INSERT OR IGNORE INTO allowlist (contract_id, address) VALUES (?,?)",
            [e.contractId, e.address]
          );
        }
        break;
      case "AllowRemove":
        if (e.address) {
          this.db.run(
            "DELETE FROM allowlist WHERE contract_id = ? AND address = ?",
            [e.contractId, e.address]
          );
        }
        break;
      case "DenyAdd":
        if (e.address) {
          this.db.run(
            "INSERT OR IGNORE INTO denylist (contract_id, address) VALUES (?,?)",
            [e.contractId, e.address]
          );
        }
        break;
      case "DenyRemove":
        if (e.address) {
          this.db.run(
            "DELETE FROM denylist WHERE contract_id = ? AND address = ?",
            [e.contractId, e.address]
          );
        }
        break;
      case "JurisdictionSet":
        if (e.address && e.jurisdiction) {
          this.db.run(
            `INSERT INTO jurisdictions (contract_id, address, code) VALUES (?,?,?)
             ON CONFLICT (contract_id, address) DO UPDATE SET code = excluded.code`,
            [e.contractId, e.address, e.jurisdiction]
          );
        }
        break;
      case "Blocked":
        // Recorded in events log only; no materialised-state change.
        break;
      case "ComplianceEvent":
        // Append a row to the audit_log materialised table so callers can
        // query the full audit trail without scanning the raw events table.
        if (e.kind && e.address && e.source != null) {
          this.db.run(
            `INSERT INTO audit_log
               (contract_id, ledger, timestamp, kind, subject, source, detail)
             VALUES (?,?,?,?,?,?,?)`,
            [
              e.contractId,
              e.ledgerSequence,
              e.timestamp,
              e.kind,
              e.address,   // subject
              e.source,
              e.detail ?? "",
            ]
          );
        }
        break;
    }
  }

  getState(key: string): string | undefined {
    const res = this.db.exec(
      "SELECT value FROM indexer_state WHERE key = ?",
      [key]
    );
    if (res.length === 0 || res[0].values.length === 0) return undefined;
    return String(res[0].values[0][0]);
  }

  setState(key: string, value: string): void {
    this.db.run(
      `INSERT INTO indexer_state (key, value) VALUES (?,?)
       ON CONFLICT (key) DO UPDATE SET value = excluded.value`,
      [key, value]
    );
    this.flush();
  }

  /**
   * Query indexed compliance events that reference a given address (as
   * primary subject or secondary address in Blocked events).
   *
   * @param address   Stellar/Soroban address to filter by (G… or C…).
   * @param options   Optional filters:
   *   - contractId   Restrict to a specific contract.
   *   - eventType    Restrict to a specific event type (e.g. "DenyAdd").
   *   - limit        Maximum number of rows to return (default 100).
   *   - offset       Row offset for pagination (default 0).
   */
  queryEventsByAddress(
    address: string,
    options: {
      contractId?: string;
      eventType?: string;
      limit?: number;
      offset?: number;
    } = {}
  ): RawEvent[] {
    const { contractId, eventType, limit = 100, offset = 0 } = options;
    const conditions: string[] = ["(address = ? OR address_to = ?)"];
    const params: (string | number)[] = [address, address];

    if (contractId) {
      conditions.push("contract_id = ?");
      params.push(contractId);
    }
    if (eventType) {
      conditions.push("event_type = ?");
      params.push(eventType);
    }

    params.push(limit, offset);
    const sql = `
      SELECT
        id, ledger_sequence, timestamp, contract_id, event_type,
        address, address_to, amount, jurisdiction,
        kind, source, detail,
        raw_topics, raw_data,
        signer_address, new_threshold, valid_count,
        policy_from, policy_to, policy_passed
      FROM events
      WHERE ${conditions.join(" AND ")}
      ORDER BY ledger_sequence DESC, id DESC
      LIMIT ? OFFSET ?
    `;

    const result = this.db.exec(sql, params);
    if (!result.length || !result[0].values.length) return [];

    return result[0].values.map((row): RawEvent => ({
      ledgerSequence: Number(row[1]),
      timestamp:      row[2] != null ? Number(row[2]) : null,
      contractId:     String(row[3]),
      eventType:      String(row[4]),
      address:        row[5] != null ? String(row[5]) : null,
      addressTo:      row[6] != null ? String(row[6]) : null,
      amount:         row[7] != null ? String(row[7]) : null,
      jurisdiction:   row[8] != null ? String(row[8]) : null,
      kind:           row[9] != null ? String(row[9]) : null,
      source:         row[10] != null ? String(row[10]) : null,
      detail:         row[11] != null ? String(row[11]) : null,
      rawTopics:      String(row[12]),
      rawData:        String(row[13]),
      signerAddress:  row[14] != null ? String(row[14]) : null,
      newThreshold:   row[15] != null ? Number(row[15]) : null,
      validCount:     row[16] != null ? Number(row[16]) : null,
      policyFrom:     row[17] != null ? String(row[17]) : null,
      policyTo:       row[18] != null ? String(row[18]) : null,
      policyPassed:   row[19] != null ? Number(row[19]) === 1 : null,
    }));
  }

  getEventCount(contractId?: string): number {
    const result = contractId
      ? this.db.exec("SELECT COUNT(*) AS count FROM events WHERE contract_id = ?", [contractId])
      : this.db.exec("SELECT COUNT(*) AS count FROM events");
    return Number(result[0]?.values[0]?.[0] ?? 0);
  }

  isAllowlisted(contractId: string, address: string): boolean {
    const result = this.db.exec("SELECT 1 FROM allowlist WHERE contract_id = ? AND address = ?", [contractId, address]);
    return result.length > 0 && result[0].values.length > 0;
  }

  getLastIndexedLedger(): number {
    const v = this.getState("last_ledger");
    return v ? Number(v) : 0;
  }

  setLastIndexedLedger(ledger: number): void {
    this.setState("last_ledger", String(ledger));
  }

  close(): void {
    this.flush();
    this.db.close();
  }
}
