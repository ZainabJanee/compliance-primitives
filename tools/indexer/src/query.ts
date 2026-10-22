/**
 * query — CLI subcommand for looking up indexed compliance events by address.
 *
 * Usage:
 *   npm run query -- --address G...
 *   npm run query -- --address G... --contract C... --type DenyAdd --limit 50
 *   npm run query -- --address G... --json
 *
 * Flags:
 *   --address <addr>    (required) Stellar address to query (G… or C…).
 *   --db <path>         Path to compliance.db (default: $DB_PATH or ./compliance.db).
 *   --contract <id>     Restrict results to a specific contract ID.
 *   --type <eventType>  Restrict results to a specific event type.
 *   --limit <n>         Max rows to return (default: 100).
 *   --offset <n>        Row offset for pagination (default: 0).
 *   --json              Output raw JSON instead of the human-readable table.
 */

import path from "node:path";
import process from "node:process";
import { ComplianceDb, type RawEvent } from "./db.js";

// ---------------------------------------------------------------------------
// Argument parsing (no external deps — keeps the tool self-contained)
// ---------------------------------------------------------------------------

interface QueryArgs {
  address: string;
  dbPath: string;
  contractId?: string;
  eventType?: string;
  limit: number;
  offset: number;
  json: boolean;
}

function parseArgs(argv: string[]): QueryArgs {
  const args: QueryArgs = {
    address: "",
    dbPath: process.env["DB_PATH"] ?? path.join(process.cwd(), "compliance.db"),
    limit: 100,
    offset: 0,
    json: false,
  };

  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const next = argv[i + 1];

    switch (flag) {
      case "--address":
        if (!next) fatal("--address requires a value");
        args.address = next;
        i++;
        break;
      case "--db":
        if (!next) fatal("--db requires a value");
        args.dbPath = next;
        i++;
        break;
      case "--contract":
        if (!next) fatal("--contract requires a value");
        args.contractId = next;
        i++;
        break;
      case "--type":
        if (!next) fatal("--type requires a value");
        args.eventType = next;
        i++;
        break;
      case "--limit":
        if (!next) fatal("--limit requires a value");
        args.limit = Number(next);
        if (!Number.isInteger(args.limit) || args.limit < 1) fatal("--limit must be a positive integer");
        i++;
        break;
      case "--offset":
        if (!next) fatal("--offset requires a value");
        args.offset = Number(next);
        if (!Number.isInteger(args.offset) || args.offset < 0) fatal("--offset must be a non-negative integer");
        i++;
        break;
      case "--json":
        args.json = true;
        break;
      case "--help":
      case "-h":
        printHelp();
        process.exit(0);
        break;
      default:
        if (flag.startsWith("--")) fatal(`Unknown flag: ${flag}`);
    }
  }

  if (!args.address) {
    fatal("--address is required");
  }

  return args;
}

function fatal(msg: string): never {
  console.error(`error: ${msg}`);
  console.error("Run with --help for usage.");
  process.exit(1);
}

function printHelp(): void {
  console.log(`
compliance-indexer query — look up indexed compliance events by address

Usage:
  npm run query -- --address <G...> [options]
  node dist/query.js --address <G...> [options]

Required:
  --address <addr>     Stellar address to search for (G… or C…)

Optional:
  --db <path>          Path to compliance.db  (default: $DB_PATH or ./compliance.db)
  --contract <id>      Filter by contract ID
  --type <eventType>   Filter by event type   (AllowAdd|AllowRemove|DenyAdd|DenyRemove|
                                               JurisdictionSet|Blocked|ComplianceEvent)
  --limit <n>          Max results to return  (default: 100)
  --offset <n>         Row offset             (default: 0)
  --json               Output raw JSON
  --help, -h           Show this help
`.trim());
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

function formatTimestamp(ts: number | null): string {
  if (ts == null) return "—";
  return new Date(ts * 1000).toISOString().replace("T", " ").replace(".000Z", " UTC");
}

function formatEvent(e: RawEvent): string {
  const lines: string[] = [];
  lines.push(`  ledger   : ${e.ledgerSequence}`);
  lines.push(`  time     : ${formatTimestamp(e.timestamp)}`);
  lines.push(`  contract : ${e.contractId}`);
  lines.push(`  type     : ${e.eventType}`);
  if (e.address)      lines.push(`  address  : ${e.address}`);
  if (e.addressTo)    lines.push(`  to       : ${e.addressTo}`);
  if (e.amount)       lines.push(`  amount   : ${e.amount}`);
  if (e.jurisdiction) lines.push(`  juris.   : ${e.jurisdiction}`);
  if (e.kind)         lines.push(`  kind     : ${e.kind}`);
  if (e.source)       lines.push(`  source   : ${e.source}`);
  if (e.detail)       lines.push(`  detail   : ${e.detail}`);
  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

export async function runQuery(argv: string[] = process.argv.slice(2)): Promise<void> {
  const args = parseArgs(argv);

  let db: ComplianceDb;
  try {
    db = await ComplianceDb.open(args.dbPath);
  } catch (err) {
    fatal(`Could not open database at ${args.dbPath}: ${err instanceof Error ? err.message : String(err)}`);
  }

  let events: RawEvent[];
  try {
    events = db.queryEventsByAddress(args.address, {
      contractId: args.contractId,
      eventType:  args.eventType,
      limit:      args.limit,
      offset:     args.offset,
    });
  } finally {
    db.close();
  }

  if (args.json) {
    console.log(JSON.stringify(events, null, 2));
    return;
  }

  const total = events.length;
  if (total === 0) {
    console.log(`No compliance events found for address: ${args.address}`);
    return;
  }

  const plural = total === 1 ? "event" : "events";
  console.log(`\nFound ${total} compliance ${plural} for ${args.address}` +
    (args.offset ? ` (offset ${args.offset})` : "") + "\n");
  console.log("─".repeat(72));

  for (const event of events) {
    console.log(formatEvent(event));
    console.log("─".repeat(72));
  }

  if (total === args.limit) {
    console.log(`\nShowing first ${args.limit} results. Use --offset ${args.offset + args.limit} to see more.`);
  }
}
