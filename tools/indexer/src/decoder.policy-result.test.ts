/**
 * Unit tests for decoder.ts — policy-engine PolicyResult event.
 *
 * The policy-engine emits a PolicyResult event on every call to `evaluate`,
 * regardless of whether the policy passed or failed.  The wire format is:
 *
 *   topics: [Symbol("PolicyResult"), Bool(passed)]
 *   data:   Vec[Address(from), Address(to)]
 *
 * This test file validates that decodeEvent correctly maps that shape to a
 * RawEvent with:
 *   eventType  = "PolicyResult"
 *   policyPassed = true | false
 *   policyFrom   = G-address string
 *   policyTo     = G-address string
 *   address / addressTo / amount / jurisdiction = null
 *
 * XDR layout recap for values encoded here:
 *   ScVal::Symbol(s)  → [u32 disc=15] [u32 len] [bytes] [pad to 4]
 *   ScVal::Bool(b)    → [u32 disc=0]  [u32 0-or-1]
 *   ScVal::Vec        → [u32 disc=16] [u32 1=Some] [u32 len] [elements...]
 *   ScVal::Address(Account) → [u32 disc=18] [u32 0] [u32 0] [32 bytes]
 *
 * Run with:
 *   npx tsx --test src/decoder.policy-result.test.ts
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

function xdrBool(value: boolean): Uint8Array {
  // ScVal::Bool discriminant = 0, then u32 0-or-1
  return concat(u32(0), u32(value ? 1 : 0));
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
  const parts: Uint8Array[] = [u32(16), u32(1), u32(items.length)];
  for (const item of items) parts.push(item);
  return concat(...parts);
}

function toBase64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64");
}

// ─── Test fixtures ────────────────────────────────────────────────────────────

// 32-byte deterministic keys
const KEY_FROM = new Uint8Array(32).fill(0xaa);
const KEY_TO   = new Uint8Array(32).fill(0xbb);

const CONTRACT_ID  = "CPOLICYENGINE000000000000000000000000000000000001";
const LEDGER_SEQ   = 7500;
const CLOSED_AT    = "2025-09-01T00:00:00Z";
const EXPECTED_TS  = Math.floor(new Date(CLOSED_AT).getTime() / 1000);

// Pre-encode the fixed topics and data
const T0  = toBase64(xdrSymbol("PolicyResult"));
const T1_PASS = toBase64(xdrBool(true));
const T1_FAIL = toBase64(xdrBool(false));
const DATA = toBase64(xdrVec([xdrAccountAddress(KEY_FROM), xdrAccountAddress(KEY_TO)]));

function makeRaw(
  t1: string,
  overrides: Partial<RawSorobanEvent> = {}
): RawSorobanEvent {
  return {
    type: "contract",
    ledger: LEDGER_SEQ,
    ledgerClosedAt: CLOSED_AT,
    contractId: CONTRACT_ID,
    id: "0001-policy",
    pagingToken: "0001-policy",
    inSuccessfulContractCall: true,
    topic: [T0, t1],
    value: DATA,
    ...overrides,
  };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("decodeEvent — policy-engine PolicyResult", () => {
  // ── Passing evaluation ─────────────────────────────────────────────────────
  describe("PolicyResult passed=true", () => {
    it("decodes eventType as PolicyResult", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null, "expected a non-null decoded event");
      assert.equal(result.eventType, "PolicyResult");
    });

    it("decodes policyPassed as true", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.equal(result.policyPassed, true);
    });

    it("decodes policyFrom as a G-address", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.ok(result.policyFrom !== null, "policyFrom should not be null");
      assert.match(result.policyFrom!, /^G/, "policyFrom should start with G");
    });

    it("decodes policyTo as a G-address distinct from policyFrom", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.ok(result.policyTo !== null, "policyTo should not be null");
      assert.match(result.policyTo!, /^G/, "policyTo should start with G");
      assert.notEqual(result.policyFrom, result.policyTo, "from and to should differ");
    });

    it("sets primitive address fields to null", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.equal(result.address, null);
      assert.equal(result.addressTo, null);
      assert.equal(result.amount, null);
      assert.equal(result.jurisdiction, null);
    });

    it("sets audit-log fields to null", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.equal(result.kind, null);
      assert.equal(result.source, null);
      assert.equal(result.detail, null);
    });

    it("records ledger sequence and timestamp", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.equal(result.ledgerSequence, LEDGER_SEQ);
      assert.equal(result.timestamp, EXPECTED_TS);
    });

    it("records contractId", () => {
      const result = decodeEvent(makeRaw(T1_PASS));
      assert.ok(result !== null);
      assert.equal(result.contractId, CONTRACT_ID);
    });
  });

  // ── Failing evaluation ─────────────────────────────────────────────────────
  describe("PolicyResult passed=false", () => {
    it("decodes policyPassed as false", () => {
      const result = decodeEvent(makeRaw(T1_FAIL));
      assert.ok(result !== null);
      assert.equal(result.eventType, "PolicyResult");
      assert.equal(result.policyPassed, false);
    });

    it("still decodes policyFrom and policyTo correctly", () => {
      const result = decodeEvent(makeRaw(T1_FAIL));
      assert.ok(result !== null);
      assert.ok(result.policyFrom !== null, "policyFrom must not be null");
      assert.ok(result.policyTo !== null, "policyTo must not be null");
    });
  });

  // ── Guard cases ────────────────────────────────────────────────────────────
  describe("guard cases", () => {
    it("returns null when Bool topic is missing (only 1 topic)", () => {
      const raw = makeRaw(T1_PASS, { topic: [T0] });
      assert.equal(decodeEvent(raw), null);
    });

    it("returns null when topic[1] is an Address instead of Bool", () => {
      // Topics shape for most primitive events is [Symbol, Address] — ensure
      // PolicyResult is not mistakenly decoded from a wrong shape.
      const addrTopic = toBase64(xdrAccountAddress(KEY_FROM));
      const raw = makeRaw(addrTopic);
      // "PolicyResult" with Address at topic[1] is malformed — decoder should reject it.
      assert.equal(decodeEvent(raw), null);
    });

    it("sets policyFrom and policyTo to null when data Vec is missing", () => {
      // If the data field is absent/void the decoder should still return a
      // valid event but with null address fields.
      const emptyData = toBase64(concat(u32(1))); // ScVal::Void
      const raw = makeRaw(T1_PASS, { value: emptyData });
      const result = decodeEvent(raw);
      assert.ok(result !== null, "should decode even without Vec data");
      assert.equal(result.eventType, "PolicyResult");
      assert.equal(result.policyPassed, true);
      assert.equal(result.policyFrom, null);
      assert.equal(result.policyTo, null);
    });
  });

  // ── Regression: existing events unaffected ─────────────────────────────────
  describe("regression — AllowAdd still decodes after adding PolicyResult", () => {
    it("AllowAdd is not confused with PolicyResult", () => {
      const t0 = toBase64(xdrSymbol("AllowAdd"));
      const t1 = toBase64(xdrAccountAddress(KEY_FROM));
      const data = toBase64(u32(1)); // Void

      const raw: RawSorobanEvent = {
        type: "contract",
        ledger: 1,
        ledgerClosedAt: CLOSED_AT,
        contractId: CONTRACT_ID,
        id: "rg-1",
        pagingToken: "rg-1",
        inSuccessfulContractCall: true,
        topic: [t0, t1],
        value: data,
      };

      const result = decodeEvent(raw);
      assert.ok(result !== null);
      assert.equal(result.eventType, "AllowAdd");
      assert.equal(result.policyFrom, null);
      assert.equal(result.policyTo, null);
      assert.equal(result.policyPassed, null);
    });
  });
});
