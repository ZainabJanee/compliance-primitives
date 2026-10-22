/**
 * Unit tests for decoder.ts — multisig-admin signer/threshold events.
 *
 * The multisig-admin contract uses symbol_short! names and emits four event
 * types (from contracts/multisig-admin/src/lib.rs):
 *
 *   SignerAdd  topics: [Symbol("SignerAdd"), Address(signer)]  data: Void
 *              Emitted when add_signer() succeeds.
 *
 *   SignerRm   topics: [Symbol("SignerRm"),  Address(signer)]  data: Void
 *              Emitted when remove_signer() succeeds.
 *
 *   ThreshSet  topics: [Symbol("ThreshSet")]                   data: U32(threshold)
 *              Emitted when update_threshold() succeeds.
 *
 *   AuthOk     topics: [Symbol("AuthOk")]
 *              data:   Vec[U32(valid_count), U32(threshold)]
 *              Emitted on every successful __check_auth call.
 *
 * These map to the following RawEvent fields:
 *   signerAddress  — populated for SignerAdd / SignerRm
 *   newThreshold   — populated for ThreshSet / AuthOk
 *   validCount     — populated for AuthOk
 *
 * XDR layout recap for values encoded here:
 *   ScVal::Symbol(s)  → [u32 disc=15] [u32 len] [bytes] [pad to 4]
 *   ScVal::Void       → [u32 disc=1]
 *   ScVal::U32(n)     → [u32 disc=3]  [u32 n]
 *   ScVal::Vec        → [u32 disc=16] [u32 1=Some] [u32 len] [elements...]
 *   ScVal::Address(Account) → [u32 disc=18] [u32 0] [u32 0] [32 bytes]
 *
 * Run with:
 *   npx tsx --test src/decoder.multisig-admin.test.ts
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { decodeEvent } from "./decoder.js";
import type { RawSorobanEvent } from "./rpc.js";

// ─── XDR write helpers ────────────────────────────────────────────────────────

function u32(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, false);
  return b;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let pos = 0;
  for (const p of parts) {
    out.set(p, pos);
    pos += p.length;
  }
  return out;
}

function xdrSymbol(s: string): Uint8Array {
  const enc = new TextEncoder().encode(s);
  const pad = (4 - (enc.length % 4)) % 4;
  return concat(u32(15), u32(enc.length), enc, new Uint8Array(pad));
}

function xdrVoid(): Uint8Array {
  return u32(1); // ScVal::Void discriminant
}

function xdrU32(n: number): Uint8Array {
  // ScVal::U32 discriminant=3, then the u32 value
  return concat(u32(3), u32(n));
}

function xdrAccountAddress(pubkey: Uint8Array): Uint8Array {
  if (pubkey.length !== 32) throw new Error("pubkey must be 32 bytes");
  return concat(
    u32(18), // ScVal::Address
    u32(0),  // ScAddressType::Account
    u32(0),  // PublicKey::ED25519
    pubkey
  );
}

function xdrVec(items: Uint8Array[]): Uint8Array {
  // ScVal::Vec → disc=16, Some=1, count, items...
  return concat(u32(16), u32(1), u32(items.length), ...items);
}

function toBase64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64");
}

// ─── Test fixtures ────────────────────────────────────────────────────────────

const SIGNER_KEY = new Uint8Array(32).fill(0xaa);
const CONTRACT_ID = "CMULTISIGADMIN0000000000000000000000000000000001";
const LEDGER_SEQ  = 9000;
const CLOSED_AT   = "2025-10-01T00:00:00Z";
const EXPECTED_TS = Math.floor(new Date(CLOSED_AT).getTime() / 1000);

const THRESHOLD = 2;
const VALID_COUNT = 3;

// Pre-encoded topics and data
const SIGNER_ADD_T0 = toBase64(xdrSymbol("SignerAdd"));
const SIGNER_RM_T0  = toBase64(xdrSymbol("SignerRm"));
const THRESH_SET_T0 = toBase64(xdrSymbol("ThreshSet"));
const AUTH_OK_T0    = toBase64(xdrSymbol("AuthOk"));
const SIGNER_T1     = toBase64(xdrAccountAddress(SIGNER_KEY));
const VOID_DATA     = toBase64(xdrVoid());
const THRESH_DATA   = toBase64(xdrU32(THRESHOLD));
const AUTH_OK_DATA  = toBase64(xdrVec([xdrU32(VALID_COUNT), xdrU32(THRESHOLD)]));

function makeRaw(
  topic: string[],
  value: string,
  overrides: Partial<RawSorobanEvent> = {}
): RawSorobanEvent {
  return {
    type: "contract",
    ledger: LEDGER_SEQ,
    ledgerClosedAt: CLOSED_AT,
    contractId: CONTRACT_ID,
    id: "0001-multisig",
    pagingToken: "0001-multisig",
    inSuccessfulContractCall: true,
    topic,
    value,
    ...overrides,
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("decodeEvent — multisig-admin events", () => {
  // ── SignerAdd ──────────────────────────────────────────────────────────────
  describe("SignerAdd", () => {
    it("decodes eventType as SignerAdd", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null, "expected non-null decoded event");
      assert.equal(result.eventType, "SignerAdd");
    });

    it("populates signerAddress as a G-address", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.ok(result.signerAddress !== null, "signerAddress must not be null");
      assert.match(result.signerAddress!, /^G/, "signerAddress should start with G");
    });

    it("sets threshold and validCount to null", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.equal(result.newThreshold, null);
      assert.equal(result.validCount, null);
    });

    it("sets address and other primitive fields to null", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.equal(result.address, null);
      assert.equal(result.addressTo, null);
      assert.equal(result.amount, null);
      assert.equal(result.jurisdiction, null);
    });

    it("records ledger sequence, timestamp, and contractId", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.equal(result.ledgerSequence, LEDGER_SEQ);
      assert.equal(result.timestamp, EXPECTED_TS);
      assert.equal(result.contractId, CONTRACT_ID);
    });

    it("returns null when signer address topic is missing", () => {
      const result = decodeEvent(makeRaw([SIGNER_ADD_T0], VOID_DATA));
      assert.equal(result, null);
    });
  });

  // ── SignerRm ───────────────────────────────────────────────────────────────
  describe("SignerRm", () => {
    it("decodes eventType as SignerRm", () => {
      const result = decodeEvent(makeRaw([SIGNER_RM_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.equal(result.eventType, "SignerRm");
    });

    it("populates signerAddress as a G-address", () => {
      const result = decodeEvent(makeRaw([SIGNER_RM_T0, SIGNER_T1], VOID_DATA));
      assert.ok(result !== null);
      assert.ok(result.signerAddress !== null, "signerAddress must not be null");
      assert.match(result.signerAddress!, /^G/);
    });

    it("same signerAddress as SignerAdd for the same key", () => {
      const addResult = decodeEvent(makeRaw([SIGNER_ADD_T0, SIGNER_T1], VOID_DATA));
      const rmResult  = decodeEvent(makeRaw([SIGNER_RM_T0,  SIGNER_T1], VOID_DATA));
      assert.ok(addResult !== null && rmResult !== null);
      assert.equal(addResult.signerAddress, rmResult.signerAddress);
    });

    it("returns null when signer address topic is missing", () => {
      const result = decodeEvent(makeRaw([SIGNER_RM_T0], VOID_DATA));
      assert.equal(result, null);
    });
  });

  // ── ThreshSet ──────────────────────────────────────────────────────────────
  describe("ThreshSet", () => {
    it("decodes eventType as ThreshSet", () => {
      const result = decodeEvent(makeRaw([THRESH_SET_T0], THRESH_DATA));
      assert.ok(result !== null, "expected non-null decoded event");
      assert.equal(result.eventType, "ThreshSet");
    });

    it("populates newThreshold with the correct value", () => {
      const result = decodeEvent(makeRaw([THRESH_SET_T0], THRESH_DATA));
      assert.ok(result !== null);
      assert.equal(result.newThreshold, THRESHOLD);
    });

    it("sets signerAddress and validCount to null", () => {
      const result = decodeEvent(makeRaw([THRESH_SET_T0], THRESH_DATA));
      assert.ok(result !== null);
      assert.equal(result.signerAddress, null);
      assert.equal(result.validCount, null);
    });

    it("returns null when data is not U32", () => {
      // Void data instead of U32 — decoder must reject this
      const result = decodeEvent(makeRaw([THRESH_SET_T0], VOID_DATA));
      assert.equal(result, null);
    });

    it("records ledger metadata correctly", () => {
      const result = decodeEvent(makeRaw([THRESH_SET_T0], THRESH_DATA));
      assert.ok(result !== null);
      assert.equal(result.ledgerSequence, LEDGER_SEQ);
      assert.equal(result.timestamp, EXPECTED_TS);
    });
  });

  // ── AuthOk ─────────────────────────────────────────────────────────────────
  describe("AuthOk", () => {
    it("decodes eventType as AuthOk", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null, "expected non-null decoded event");
      assert.equal(result.eventType, "AuthOk");
    });

    it("populates validCount correctly", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null);
      assert.equal(result.validCount, VALID_COUNT);
    });

    it("populates newThreshold correctly", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null);
      assert.equal(result.newThreshold, THRESHOLD);
    });

    it("sets signerAddress to null", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null);
      assert.equal(result.signerAddress, null);
    });

    it("sets primitive address fields to null", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null);
      assert.equal(result.address, null);
      assert.equal(result.addressTo, null);
    });

    it("gracefully sets validCount/newThreshold to null when data Vec is malformed", () => {
      // Void data — Vec with 0 items instead of 2
      const result = decodeEvent(makeRaw([AUTH_OK_T0], VOID_DATA));
      assert.ok(result !== null, "should still return an event (just with null counts)");
      assert.equal(result.eventType, "AuthOk");
      assert.equal(result.validCount, null);
      assert.equal(result.newThreshold, null);
    });

    it("records ledger metadata correctly", () => {
      const result = decodeEvent(makeRaw([AUTH_OK_T0], AUTH_OK_DATA));
      assert.ok(result !== null);
      assert.equal(result.ledgerSequence, LEDGER_SEQ);
      assert.equal(result.timestamp, EXPECTED_TS);
    });
  });

  // ── Regression: existing events not confused with multisig events ──────────
  describe("regression — existing events unaffected", () => {
    it("DenyAdd is not confused with SignerAdd or SignerRm", () => {
      const t0 = toBase64(xdrSymbol("DenyAdd"));
      const t1 = toBase64(xdrAccountAddress(SIGNER_KEY));
      const result = decodeEvent(makeRaw([t0, t1], VOID_DATA));
      assert.ok(result !== null);
      assert.equal(result.eventType, "DenyAdd");
      assert.equal(result.signerAddress, null);
      assert.equal(result.newThreshold, null);
      assert.equal(result.validCount, null);
    });

    it("PolicyResult is not confused with ThreshSet or AuthOk", () => {
      // PolicyResult: [Symbol("PolicyResult"), Bool(true)], data: Vec[Addr, Addr]
      const pr_t0 = toBase64(xdrSymbol("PolicyResult"));
      const pr_t1 = toBase64(concat(u32(0), u32(1))); // Bool(true)
      const pr_data = toBase64(xdrVec([
        xdrAccountAddress(new Uint8Array(32).fill(0xbb)),
        xdrAccountAddress(new Uint8Array(32).fill(0xcc)),
      ]));
      const result = decodeEvent(makeRaw([pr_t0, pr_t1], pr_data));
      assert.ok(result !== null);
      assert.equal(result.eventType, "PolicyResult");
      assert.equal(result.signerAddress, null);
      assert.equal(result.newThreshold, null);
    });
  });
});
