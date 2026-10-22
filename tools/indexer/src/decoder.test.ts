/**
 * Unit tests for decoder.ts — ComplianceEvent (audit-log) decoding.
 *
 * Run with:
 *   npx tsx --test src/decoder.test.ts
 *
 * We construct minimal hand-crafted XDR byte sequences for each ScVal, then
 * base64-encode them to match the format the Soroban RPC returns.  No
 * external XDR library is required; the helpers here mirror the write side of
 * the XdrReader used in production.
 *
 * XDR layout recap for the values we encode:
 *
 *   ScVal::Symbol(s)        → [u32 disc=15] [u32 len] [bytes] [pad to 4]
 *   ScVal::String(s)        → [u32 disc=14] [u32 len] [bytes] [pad to 4]
 *   ScVal::Address(Account) → [u32 disc=18] [u32 addrType=0]
 *                               [u32 pubkeyType=0] [32 bytes]
 *   ScVal::Map              → [u32 disc=17] [u32 some=1] [u32 count]
 *                               for each entry: key ScVal, value ScVal
 *   ScVal::Void             → [u32 disc=1]
 *
 * All multi-byte integers are big-endian (XDR spec §3.1).
 * Variable-length opaque data is padded to a 4-byte boundary.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeEvent } from "./decoder.js";
import type { RawSorobanEvent } from "./rpc.js";

// ─── XDR write helpers ────────────────────────────────────────────────────────

function u32(n: number): number[] {
  return [(n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff];
}

function varBytes(data: number[]): number[] {
  const pad = (4 - (data.length % 4)) % 4;
  return [...u32(data.length), ...data, ...new Array(pad).fill(0)];
}

function xdrStringLike(disc: number, s: string): number[] {
  const encoded = Array.from(new TextEncoder().encode(s));
  return [...u32(disc), ...varBytes(encoded)];
}

/** XDR-encode ScVal::Symbol */
function xdrSymbol(s: string): number[] {
  return xdrStringLike(15, s);
}

/** XDR-encode ScVal::String */
function xdrStr(s: string): number[] {
  return xdrStringLike(14, s);
}

function xdrBool(value: boolean): number[] {
  return [...u32(0), ...u32(value ? 1 : 0)];
}

function xdrU32(value: number): number[] {
  return [...u32(3), ...u32(value)];
}

function xdrVec(values: number[][]): number[] {
  return [...u32(16), ...u32(1), ...u32(values.length), ...values.flat()];
}

/**
 * XDR-encode ScVal::Address for an Account (Ed25519 public key).
 * @param pubkey 32-byte Ed25519 public key
 */
function xdrAccountAddress(pubkey: Uint8Array): number[] {
  if (pubkey.length !== 32) throw new Error("pubkey must be 32 bytes");
  return [
    ...u32(18), // ScVal::Address discriminant
    ...u32(0),  // ScAddressType::Account
    ...u32(0),  // PublicKey::ED25519 discriminant
    ...Array.from(pubkey),
  ];
}

/**
 * XDR-encode ScVal::Map.
 * @param entries array of pre-encoded [key, value] byte sequences concatenated
 */
function xdrMap(entries: number[][]): number[] {
  return [
    ...u32(17), // ScVal::Map discriminant
    ...u32(1),  // Some (non-null map)
    ...u32(entries.length),
    ...entries.flat(),
  ];
}

function bytesToBase64(bytes: number[]): string {
  return btoa(String.fromCharCode(...bytes));
}

// ─── Test data ────────────────────────────────────────────────────────────────

// Deterministic 32-byte "addresses" for reproducible tests.
const SUBJECT_KEY = new Uint8Array(32).fill(0xab);
const SOURCE_KEY = new Uint8Array(32).fill(0xcd);

const CONTRACT_ID = "CCOMPLIANCE0000000000000000000000000000000000000000000001";
const KIND = "deny_add";
const DETAIL = "added by compliance officer";

// ─── Fixture builder ──────────────────────────────────────────────────────────

function buildComplianceEventRaw(opts: {
  kind?: string;
  detail?: string;
  subjectKey?: Uint8Array;
  sourceKey?: Uint8Array;
  ledger?: number;
  ledgerClosedAt?: string;
}): RawSorobanEvent {
  const {
    kind = KIND,
    detail = DETAIL,
    subjectKey = SUBJECT_KEY,
    sourceKey = SOURCE_KEY,
    ledger = 1234,
    ledgerClosedAt = "2025-01-01T00:00:00Z",
  } = opts;

  const topic0 = bytesToBase64(xdrSymbol(kind));
  const topic1 = bytesToBase64(xdrAccountAddress(subjectKey));

  // data = Map { "source": Address(sourceKey), "detail": String(detail) }
  const sourceEntry = [...xdrSymbol("source"), ...xdrAccountAddress(sourceKey)];
  const detailEntry = [...xdrSymbol("detail"), ...xdrStr(detail)];
  const dataB64 = bytesToBase64(xdrMap([sourceEntry, detailEntry]));

  return {
    type: "contract",
    ledger,
    ledgerClosedAt,
    contractId: CONTRACT_ID,
    topic: [topic0, topic1],
    value: dataB64,
    id: "test-id",
    pagingToken: "test-paging-token",
    inSuccessfulContractCall: true,
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

test("decodes a ComplianceEvent to the expected shape", () => {
  const raw = buildComplianceEventRaw({});
  const result = decodeEvent(raw);

  assert.notEqual(result, null, "decodeEvent should not return null");
  assert.strictEqual(result!.eventType, "ComplianceEvent");
  assert.strictEqual(result!.kind, KIND);

  // subject address → result.address; must be a G-address
  assert.notEqual(result!.address, null);
  assert.match(result!.address!, /^G/, "subject address should be a G-address");

  // source address
  assert.notEqual(result!.source, null);
  assert.match(result!.source!, /^G/, "source address should be a G-address");

  // subject and source encode different public keys
  assert.notEqual(result!.address, result!.source, "subject and source should differ");

  assert.strictEqual(result!.detail, DETAIL);

  // primitive-only fields must be null
  assert.strictEqual(result!.addressTo, null);
  assert.strictEqual(result!.amount, null);
  assert.strictEqual(result!.jurisdiction, null);

  // ledger metadata
  assert.strictEqual(result!.ledgerSequence, 1234);
  assert.strictEqual(result!.timestamp, 1735689600); // 2025-01-01T00:00:00Z

  assert.strictEqual(result!.contractId, CONTRACT_ID);

  // rawTopics should round-trip
  assert.ok(
    result!.rawTopics.includes(raw.topic[0]),
    "rawTopics should include the first topic base64"
  );
});

test("decodes ComplianceEvent with various kind values", () => {
  for (const kind of ["deny_add", "deny_remove", "jurisdiction_set", "allow_add"]) {
    const raw = buildComplianceEventRaw({ kind });
    const result = decodeEvent(raw);

    assert.notEqual(result, null, `should decode event with kind "${kind}"`);
    assert.strictEqual(result!.eventType, "ComplianceEvent", `eventType for kind "${kind}"`);
    assert.strictEqual(result!.kind, kind, `kind field for "${kind}"`);
  }
});

test("does not misidentify AllowAdd as ComplianceEvent", () => {
  // AllowAdd: topics=[Symbol("AllowAdd"), Address], data=Void
  const topic0 = bytesToBase64(xdrSymbol("AllowAdd"));
  const topic1 = bytesToBase64(xdrAccountAddress(SUBJECT_KEY));
  const dataB64 = bytesToBase64(u32(1)); // ScVal::Void

  const raw: RawSorobanEvent = {
    type: "contract",
    ledger: 100,
    ledgerClosedAt: "2025-01-01T00:00:00Z",
    contractId: CONTRACT_ID,
    topic: [topic0, topic1],
    value: dataB64,
    id: "test-id-2",
    pagingToken: "token-2",
    inSuccessfulContractCall: true,
  };

  const result = decodeEvent(raw);
  assert.notEqual(result, null, "should decode AllowAdd");
  assert.strictEqual(result!.eventType, "AllowAdd");
  assert.strictEqual(result!.kind, null, "kind should be null for primitive events");
  assert.strictEqual(result!.source, null, "source should be null for primitive events");
  assert.strictEqual(result!.detail, null, "detail should be null for primitive events");
});

test("returns null for events with fewer than 2 topics", () => {
  const raw: RawSorobanEvent = {
    type: "contract",
    ledger: 1,
    ledgerClosedAt: "2025-01-01T00:00:00Z",
    contractId: CONTRACT_ID,
    topic: [bytesToBase64(xdrSymbol("deny_add"))],
    value: "",
    id: "test-id-3",
    pagingToken: "token-3",
    inSuccessfulContractCall: true,
  };

  const result = decodeEvent(raw);
  assert.strictEqual(result, null, "single-topic event should decode to null");
});

test("does not misidentify DenyAdd (Symbol+Address+Void data) as ComplianceEvent", () => {
  // DenyAdd: topics=[Symbol("DenyAdd"), Address], data=Void — Void is not a Map
  const topic0 = bytesToBase64(xdrSymbol("DenyAdd"));
  const topic1 = bytesToBase64(xdrAccountAddress(SUBJECT_KEY));
  const dataB64 = bytesToBase64(u32(1)); // ScVal::Void

  const raw: RawSorobanEvent = {
    type: "contract",
    ledger: 1,
    ledgerClosedAt: "2025-01-01T00:00:00Z",
    contractId: CONTRACT_ID,
    topic: [topic0, topic1],
    value: dataB64,
    id: "test-id-4",
    pagingToken: "token-4",
    inSuccessfulContractCall: true,
  };

  const result = decodeEvent(raw);
  assert.notEqual(result, null, "should decode DenyAdd");
  assert.strictEqual(result!.eventType, "DenyAdd");
});

test("handles an empty detail string", () => {
  const raw = buildComplianceEventRaw({ detail: "" });
  const result = decodeEvent(raw);

  assert.notEqual(result, null);
  assert.strictEqual(result!.eventType, "ComplianceEvent");
  assert.strictEqual(result!.detail, "");
});

function buildNamedEventRaw(name: string, topics: number[][], data: number[]): RawSorobanEvent {
  return {
    type: "contract",
    ledger: 1234,
    ledgerClosedAt: "2025-01-01T00:00:00Z",
    contractId: CONTRACT_ID,
    topic: [xdrSymbol(name), ...topics].map(bytesToBase64),
    value: bytesToBase64(data),
    id: "named-event",
    pagingToken: "named-event-token",
    inSuccessfulContractCall: true,
  };
}

test("decodes policy-engine PolicyResult fields", () => {
  const raw = buildNamedEventRaw(
    "PolicyResult",
    [xdrBool(true)],
    xdrVec([xdrAccountAddress(SUBJECT_KEY), xdrAccountAddress(SOURCE_KEY)])
  );
  const result = decodeEvent(raw);

  assert.notEqual(result, null);
  assert.equal(result!.eventType, "PolicyResult");
  assert.equal(result!.policyPassed, true);
  assert.match(result!.policyFrom!, /^G/);
  assert.match(result!.policyTo!, /^G/);
  assert.equal(result!.address, null);
});

test("decodes multisig-admin signer and threshold events", () => {
  const signerAdded = decodeEvent(buildNamedEventRaw(
    "SignerAdd",
    [xdrAccountAddress(SUBJECT_KEY)],
    u32(1)
  ));
  assert.notEqual(signerAdded, null);
  assert.equal(signerAdded!.eventType, "SignerAdd");
  assert.match(signerAdded!.signerAddress!, /^G/);

  const signerRemoved = decodeEvent(buildNamedEventRaw(
    "SignerRm",
    [xdrAccountAddress(SOURCE_KEY)],
    u32(1)
  ));
  assert.notEqual(signerRemoved, null);
  assert.equal(signerRemoved!.eventType, "SignerRm");
  assert.match(signerRemoved!.signerAddress!, /^G/);

  const thresholdUpdated = decodeEvent(buildNamedEventRaw("ThreshSet", [], xdrU32(2)));
  assert.notEqual(thresholdUpdated, null);
  assert.equal(thresholdUpdated!.eventType, "ThreshSet");
  assert.equal(thresholdUpdated!.newThreshold, 2);

  const authApproved = decodeEvent(buildNamedEventRaw("AuthOk", [], xdrVec([xdrU32(3), xdrU32(2)])));
  assert.notEqual(authApproved, null);
  assert.equal(authApproved!.eventType, "AuthOk");
  assert.equal(authApproved!.validCount, 3);
  assert.equal(authApproved!.newThreshold, 2);
});
