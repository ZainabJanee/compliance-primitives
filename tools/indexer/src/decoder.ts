/**
 * Decodes raw Soroban event topics/data (base64-encoded XDR) into typed
 * compliance events.
 *
 * Soroban #[contractevent] structs with #[topic] fields are published as
 * a topic array where:
 *   topic[0]  = ScVal::Symbol — the event name (e.g. "AllowAdd")
 *   topic[1+] = the fields annotated #[topic] in declaration order
 *   data      = ScVal — struct-value encoding of any non-topic fields
 *
 * For primitive and circuit-breaker events:
 *
 *   AllowAdd        topics: [Symbol("AllowAdd"), Address]          data: Void
 *   AllowRemove     topics: [Symbol("AllowRemove"), Address]        data: Void
 *   Blocked         topics: [Symbol("Blocked"), Address, Address]   data: I128
 *   DenyAdd         topics: [Symbol("DenyAdd"), Address]            data: Void
 *   DenyRemove      topics: [Symbol("DenyRemove"), Address]         data: Void
 *   JurisdictionSet topics: [Symbol("JurisdictionSet"), Address]    data: String(code)
 *   Frozen          topics: [Symbol("Frozen"), Address(admin)]       data: Void
 *   Unfrozen        topics: [Symbol("Unfrozen"), Address(admin)]     data: Void
 *
 * For policy-engine:
 *
 *   PolicyResult topics: [Symbol("PolicyResult"), Bool(passed)]
 *               data:   Vec[Address(from), Address(to)]
 *
 * For multisig-admin:
 *
 *   SignerAdd / SignerRm topics: [Symbol(name), Address(signer)] data: Void
 *   ThreshSet            topics: [Symbol("ThreshSet")]          data: U32(threshold)
 *   AuthOk               topics: [Symbol("AuthOk")]             data: Vec[U32(valid_count), U32(threshold)]
 *
 * For the audit-log contract's ComplianceEvent:
 *
 *   ComplianceEvent topics: [Symbol(kind), Address(subject)]
 *                   data  : Map { "source": Address, "detail": String }
 *
 *   The audit-log event has NO struct-name prefix topic — the two #[topic]
 *   fields (kind, subject) fill the topic array directly.  The first topic
 *   is a Symbol whose value is the event kind recorded by the caller (e.g.
 *   "deny_add"), and the second topic is the subject address.  The data map
 *   carries the source address and a detail string.
 *
 *   Because the kind symbol overlaps with the existing primitive event names
 *   ("deny_add", "deny_remove", …), audit-log events are identified by
 *   detecting the presence of the source/detail data map rather than by a
 *   unique topic[0] name.  The decoded event is stored with eventType
 *   "ComplianceEvent" and the kind value in the `kind` field of the
 *   extended RawEvent.
 *
 * We parse XDR manually using DataView — no external XDR lib — because the
 * values we need are simple enough and we want zero extra dependencies.
 * If you need full XDR fidelity, swap in @stellar/stellar-base.
 */

import type { RawSorobanEvent } from "./rpc.js";
import type { RawEvent } from "./db.js";

// ─── Minimal XDR helpers ─────────────────────────────────────────────────────

/** XDR ScVal type discriminants we care about */
const enum ScType {
  Bool = 0,
  Void = 1,
  Error = 2,
  U32 = 3,
  I32 = 4,
  U64 = 5,
  I64 = 6,
  Timepoint = 7,
  Duration = 8,
  U128 = 9,
  I128 = 10,
  U256 = 11,
  I256 = 12,
  Bytes = 13,
  String = 14,
  Symbol = 15,
  Vec = 16,
  Map = 17,
  Address = 18,
  LedgerKeyContractInstance = 19,
  LedgerKeyNonce = 20,
  ContractInstance = 21,
}

/** Address sub-types in XDR */
const enum ScAddressType {
  Account = 0,
  Contract = 1,
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

class XdrReader {
  private view: DataView;
  private pos: number;

  constructor(bytes: Uint8Array) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    this.pos = 0;
  }

  readU32(): number {
    const v = this.view.getUint32(this.pos);
    this.pos += 4;
    return v;
  }

  readI32(): number {
    const v = this.view.getInt32(this.pos);
    this.pos += 4;
    return v;
  }

  readI64(): bigint {
    const hi = BigInt(this.view.getInt32(this.pos));
    const lo = BigInt(this.view.getUint32(this.pos + 4));
    this.pos += 8;
    return (hi << 32n) | lo;
  }

  readU64(): bigint {
    const hi = BigInt(this.view.getUint32(this.pos));
    const lo = BigInt(this.view.getUint32(this.pos + 4));
    this.pos += 8;
    return (hi << 32n) | lo;
  }

  readBytes(n: number): Uint8Array {
    const slice = new Uint8Array(
      this.view.buffer,
      this.view.byteOffset + this.pos,
      n
    );
    this.pos += n;
    // XDR pads to 4-byte boundary
    const pad = (4 - (n % 4)) % 4;
    this.pos += pad;
    return slice;
  }

  readVarBytes(): Uint8Array {
    const len = this.readU32();
    return this.readBytes(len);
  }

  readString(): string {
    return new TextDecoder().decode(this.readVarBytes());
  }
}

// ─── ScVal decoder ────────────────────────────────────────────────────────────

type ScVal =
  | { type: "Void" }
  | { type: "Bool"; value: boolean }
  | { type: "Symbol"; value: string }
  | { type: "String"; value: string }
  | { type: "Address"; value: string }
  | { type: "I128"; value: bigint }
  | { type: "U32"; value: number }
  | { type: "Vec"; value: ScVal[] }
  | { type: "Map"; value: Map<string, ScVal> }
  | { type: "Other"; discriminant: number };

function decodeScVal(r: XdrReader): ScVal {
  const disc = r.readU32();
  switch (disc) {
    case ScType.Bool:
      return { type: "Bool", value: r.readU32() !== 0 };

    case ScType.Void:
      return { type: "Void" };

    case ScType.Symbol: {
      const value = r.readString();
      return { type: "Symbol", value };
    }

    case ScType.String: {
      const value = r.readString();
      return { type: "String", value };
    }

    case ScType.U32:
      return { type: "U32", value: r.readU32() };

    case ScType.I128: {
      // hi (i64) then lo (u64)
      const hi = r.readI64();
      const lo = r.readU64();
      const value = (hi << 64n) | lo;
      return { type: "I128", value };
    }

    case ScType.Address: {
      const addrType = r.readU32();
      if (addrType === ScAddressType.Account) {
        // AccountID: PublicKey (discriminant u32 = 0) + 32 raw bytes
        r.readU32(); // PublicKey type discriminant (ED25519 = 0)
        const raw = r.readBytes(32);
        return { type: "Address", value: encodeG(raw) };
      } else {
        // Contract: 32-byte hash
        const raw = r.readBytes(32);
        return { type: "Address", value: bufToHex(raw) };
      }
    }

    case ScType.Vec: {
      // nullable union: discriminant 1 = Some, 0 = None
      const present = r.readU32();
      if (!present) return { type: "Vec", value: [] };
      const len = r.readU32();
      const items: ScVal[] = [];
      for (let i = 0; i < len; i++) items.push(decodeScVal(r));
      return { type: "Vec", value: items };
    }

    case ScType.Map: {
      // nullable union: discriminant 1 = Some, 0 = None
      const present = r.readU32();
      if (!present) return { type: "Map", value: new Map() };
      const len = r.readU32();
      const entries = new Map<string, ScVal>();
      for (let i = 0; i < len; i++) {
        const k = decodeScVal(r);
        const v = decodeScVal(r);
        // Keys in Soroban struct maps are Symbols or Strings
        const keyStr =
          k.type === "Symbol" || k.type === "String" ? k.value : String(i);
        entries.set(keyStr, v);
      }
      return { type: "Map", value: entries };
    }

    default:
      return { type: "Other", discriminant: disc };
  }
}

// ─── Address encoding helpers ────────────────────────────────────────────────

/** Hex-encode bytes (used for contract addresses) */
function bufToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/**
 * Encode a 32-byte Ed25519 public key as a Stellar G-address (StrKey).
 * We do the full StrKey base32 + checksum here so there's no dep on
 * @stellar/stellar-base.
 */
function encodeG(raw: Uint8Array): string {
  // Version byte for account (G) = 6 << 3 = 48 (0x30)
  const versionByte = 6 << 3; // 0x30
  const payload = new Uint8Array(raw.length + 1);
  payload[0] = versionByte;
  payload.set(raw, 1);
  const checksum = crc16xmodem(payload);
  const full = new Uint8Array(payload.length + 2);
  full.set(payload);
  full[payload.length] = checksum & 0xff;
  full[payload.length + 1] = (checksum >> 8) & 0xff;
  return base32Encode(full);
}

function crc16xmodem(data: Uint8Array): number {
  let crc = 0x0000;
  for (const byte of data) {
    crc ^= byte << 8;
    for (let i = 0; i < 8; i++) {
      crc = crc & 0x8000 ? (crc << 1) ^ 0x1021 : crc << 1;
      crc &= 0xffff;
    }
  }
  return crc;
}

const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
function base32Encode(data: Uint8Array): string {
  let bits = 0;
  let value = 0;
  let output = "";
  for (const byte of data) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      output += BASE32_ALPHABET[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) {
    output += BASE32_ALPHABET[(value << (5 - bits)) & 31];
  }
  return output;
}

// ─── Event decoding ───────────────────────────────────────────────────────────

const KNOWN_EVENTS = new Set([
  // compliance-primitives events
  "AllowAdd",
  "AllowRemove",
  "Blocked",
  "DenyAdd",
  "DenyRemove",
  "JurisdictionSet",
  "Frozen",
  "Unfrozen",
  // policy-engine events
  "PolicyResult",
  // multisig-admin events (symbol_short! names from the contract)
  "SignerAdd",
  "SignerRm",
  "ThreshSet",
  "AuthOk",
]);

export function decodeEvent(
  raw: RawSorobanEvent
): RawEvent | null {
  try {
    // topics is an array of base64 XDR ScVal strings
    if (!raw.topic || raw.topic.length < 1) return null;

    const topics = raw.topic.map((t) => decodeScVal(new XdrReader(base64ToBytes(t))));

    // Decode data field (may be absent / void)
    const dataVal =
      raw.value
        ? decodeScVal(new XdrReader(base64ToBytes(raw.value)))
        : { type: "Void" as const };

    const timestamp = raw.ledgerClosedAt
      ? Math.floor(new Date(raw.ledgerClosedAt).getTime() / 1000)
      : null;

    const base: RawEvent = {
      ledgerSequence: raw.ledger,
      timestamp,
      contractId: raw.contractId,
      eventType: "",
      address: null,
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
      rawTopics: JSON.stringify(raw.topic),
      rawData: raw.value ?? "",
    };

    // ── audit-log ComplianceEvent detection ──────────────────────────────────
    //
    // ComplianceEvent has NO struct-name prefix topic.  The topic array is
    // exactly [Symbol(kind), Address(subject)] and the data is a Map
    // containing "source" (Address) and "detail" (String).  We distinguish
    // it from the primitive events by checking that the data is a Map with
    // those two keys — primitive events never produce a Map as their data.
    if (
      topics.length === 2 &&
      topics[0].type === "Symbol" &&
      topics[1].type === "Address" &&
      dataVal.type === "Map"
    ) {
      const dataMap = dataVal.value;
      const sourceVal = dataMap.get("source");
      const detailVal = dataMap.get("detail");

      if (
        sourceVal?.type === "Address" &&
        (detailVal?.type === "String" || detailVal?.type === "Symbol" || detailVal == null)
      ) {
        return {
          ledgerSequence: raw.ledger,
          timestamp,
          contractId: raw.contractId,
          eventType: "ComplianceEvent",
          address: topics[1].value,                   // subject
          addressTo: null,
          amount: null,
          jurisdiction: null,
          kind: topics[0].value,                       // e.g. "deny_add"
          source: sourceVal.value,
          detail: detailVal?.type === "String" || detailVal?.type === "Symbol"
            ? detailVal.value
            : null,
          policyFrom: null,
          policyTo: null,
          policyPassed: null,
          signerAddress: null,
          newThreshold: null,
          validCount: null,
          rawTopics: JSON.stringify(raw.topic),
          rawData: raw.value ?? "",
        };
      }
    }

    // ── Primitive event decoding (name in topic[0]) ───────────────────────────
    //
    // topics[0] must be a Symbol naming the event
    const nameVal = topics[0];
    if (nameVal.type !== "Symbol") return null;
    const eventType = nameVal.value;

    // ── circuit-breaker state-change events ───────────────────────────────────
    //
    // Frozen / Unfrozen:
    //   topics: [Symbol("Frozen"|"Unfrozen"), Address(admin)]
    //   data:   Void
    //
    // topic[1] carries the admin address that triggered the change, but we
    // intentionally store address as null — the contract_id column identifies
    // which breaker instance changed state, which is the useful lookup key.
    if (eventType === "Frozen" || eventType === "Unfrozen") {
      return { ...base, eventType, address: null };
    }

    if (!KNOWN_EVENTS.has(eventType)) return null;

    // ── policy-engine PolicyResult ────────────────────────────────────────────
    //
    // PolicyResult has a different shape from the address-keyed events:
    //   topics: [Symbol("PolicyResult"), Bool(passed)]
    //   data:   Vec[Address(from), Address(to)]
    //
    // Handle it here before the section that requires topics[1] to be an
    // Address.
    if (eventType === "PolicyResult") {
      if (topics.length < 2) return null;
      const passedVal = topics[1];
      if (passedVal.type !== "Bool") return null;

      let policyFrom: string | null = null;
      let policyTo: string | null = null;

      // data is Vec[Address(from), Address(to)]
      if (dataVal.type === "Vec" && dataVal.value.length >= 2) {
        const fromVal = dataVal.value[0];
        const toVal = dataVal.value[1];
        if (fromVal.type === "Address") policyFrom = fromVal.value;
        if (toVal.type === "Address") policyTo = toVal.value;
      }

      return {
        ledgerSequence: raw.ledger,
        timestamp,
        contractId: raw.contractId,
        eventType: "PolicyResult",
        address: null,
        addressTo: null,
        amount: null,
        jurisdiction: null,
        kind: null,
        source: null,
        detail: null,
        policyFrom,
        policyTo,
        policyPassed: passedVal.value,
        signerAddress: null,
        newThreshold: null,
        validCount: null,
        rawTopics: JSON.stringify(raw.topic),
        rawData: raw.value ?? "",
      };
    }

    // ── multisig-admin signer/threshold events ────────────────────────────────
    //
    // These events use symbol_short! names and have different topic shapes:
    //
    //   SignerAdd  topics: [Symbol("SignerAdd"), Address(signer)]  data: Void
    //   SignerRm   topics: [Symbol("SignerRm"),  Address(signer)]  data: Void
    //   ThreshSet  topics: [Symbol("ThreshSet")]                   data: U32
    //   AuthOk     topics: [Symbol("AuthOk")]
    //              data:   (U32(valid_count), U32(threshold)) — Soroban encodes
    //                      a Rust tuple (u32, u32) as a two-element ScVec.
    //
    // ThreshSet and AuthOk only have 1 topic, so they must be handled before
    // the section that unconditionally reads topics[1] as an Address.

    if (eventType === "SignerAdd" || eventType === "SignerRm") {
      if (topics.length < 2) return null;
      const signerVal = topics[1];
      if (signerVal.type !== "Address") return null;
      return {
        ledgerSequence: raw.ledger,
        timestamp,
        contractId: raw.contractId,
        eventType,
        address: null,
        addressTo: null,
        amount: null,
        jurisdiction: null,
        kind: null,
        source: null,
        detail: null,
        policyFrom: null,
        policyTo: null,
        policyPassed: null,
        signerAddress: signerVal.value,
        newThreshold: null,
        validCount: null,
        rawTopics: JSON.stringify(raw.topic),
        rawData: raw.value ?? "",
      };
    }

    if (eventType === "ThreshSet") {
      if (dataVal.type !== "U32") return null;
      return {
        ledgerSequence: raw.ledger,
        timestamp,
        contractId: raw.contractId,
        eventType: "ThreshSet",
        address: null,
        addressTo: null,
        amount: null,
        jurisdiction: null,
        kind: null,
        source: null,
        detail: null,
        policyFrom: null,
        policyTo: null,
        policyPassed: null,
        signerAddress: null,
        newThreshold: dataVal.value,
        validCount: null,
        rawTopics: JSON.stringify(raw.topic),
        rawData: raw.value ?? "",
      };
    }

    if (eventType === "AuthOk") {
      // Soroban encodes the Rust tuple (u32, u32) as a two-element ScVec.
      let validCount: number | null = null;
      let newThreshold: number | null = null;
      if (dataVal.type === "Vec" && dataVal.value.length === 2) {
        const v0 = dataVal.value[0];
        const v1 = dataVal.value[1];
        if (v0.type === "U32") validCount = v0.value;
        if (v1.type === "U32") newThreshold = v1.value;
      }
      return {
        ledgerSequence: raw.ledger,
        timestamp,
        contractId: raw.contractId,
        eventType: "AuthOk",
        address: null,
        addressTo: null,
        amount: null,
        jurisdiction: null,
        kind: null,
        source: null,
        detail: null,
        policyFrom: null,
        policyTo: null,
        policyPassed: null,
        signerAddress: null,
        newThreshold,
        validCount,
        rawTopics: JSON.stringify(raw.topic),
        rawData: raw.value ?? "",
      };
    }

    // topics[1] is always the primary address for remaining event types
    const addrVal = topics[1];
    if (addrVal.type !== "Address") return null;
    const address = addrVal.value;

    let addressTo: string | null = null;
    let amount: string | null = null;
    let jurisdiction: string | null = null;

    if (eventType === "Blocked") {
      // topics[2] = to address
      const toVal = topics[2];
      if (toVal?.type === "Address") addressTo = toVal.value;
      if (dataVal.type === "I128") amount = dataVal.value.toString();
    }

    if (eventType === "JurisdictionSet") {
      if (dataVal.type === "String") jurisdiction = dataVal.value;
    }

    return {
      ...base,
      eventType,
      address,
      addressTo,
      amount,
      jurisdiction,
      kind: null,
      source: null,
      detail: null,
      policyFrom: null,
      policyTo: null,
      policyPassed: null,
      signerAddress: null,
      newThreshold: null,
      validCount: null,
      rawTopics: JSON.stringify(raw.topic),
      rawData: raw.value ?? "",
    } as const;

    if (eventType === "SignerAdd" || eventType === "SignerRm") {
      // topics: [Symbol, Address(signer)]
      if (raw.topic.length < 2) return null;
      const signerVal = topics[1];
      if (signerVal.type !== "Address") return null;
      return {
        ...base,
        signerAddress: signerVal.value,
        newThreshold: null,
        validCount: null,
      };
    }

    if (eventType === "ThreshSet") {
      // topics: [Symbol]  data: U32(threshold)
      if (dataVal.type !== "U32") return null;
      return {
        ...base,
        signerAddress: null,
        newThreshold: dataVal.value,
        validCount: null,
      };
    }

    if (eventType === "AuthOk") {
      // topics: [Symbol]  data: Vec[U32(valid_count), U32(threshold)]
      // The Soroban SDK encodes a Rust tuple (u32, u32) as a two-element ScVec.
      let validCount: number | null = null;
      let newThreshold: number | null = null;
      if (dataVal.type === "Vec" && dataVal.value.length === 2) {
        const v0 = dataVal.value[0];
        const v1 = dataVal.value[1];
        if (v0.type === "U32") validCount = v0.value;
        if (v1.type === "U32") newThreshold = v1.value;
      }
      return {
        ...base,
        signerAddress: null,
        newThreshold,
        validCount,
      };
    }

    // ── compliance-aggregator configuration events ────────────────────────────
    //
    // AdminSet, DenylistGateSet, JurisdictionFlagSet all share the same shape:
    //   topics: [Symbol(name), Address(configured_contract_or_admin)]
    //   data:   Void

    if (
      eventType === "AdminSet" ||
      eventType === "DenylistGateSet" ||
      eventType === "JurisdictionFlagSet"
    ) {
      if (raw.topic.length < 2) return null;
      const addrVal = topics[1];
      if (addrVal.type !== "Address") return null;

      return {
        ...base,
        // Reuse `address` to hold the newly configured admin/gate/flag address
        address: addrVal.value,
        signerAddress: null,
        newThreshold: null,
        validCount: null,
      };
    }

    // ── policy-engine evaluation event ───────────────────────────────────────
    //
    // PolicyResult:
    //   topics: [Symbol("PolicyResult"), Bool(passed)]
    //   data:   Vec[Address(from), Address(to)]

    if (eventType === "PolicyResult") {
      if (raw.topic.length < 2) return null;
      const passedVal = topics[1];
      if (passedVal.type !== "Bool") return null;

      let policyFrom: string | null = null;
      let policyTo: string | null = null;

      // data is Vec[Address(from), Address(to)]
      if (dataVal.type === "Vec" && dataVal.value.length >= 2) {
        const fromVal = dataVal.value[0];
        const toVal = dataVal.value[1];
        if (fromVal.type === "Address") policyFrom = fromVal.value;
        if (toVal.type === "Address") policyTo = toVal.value;
      }

      return {
        ...base,
        signerAddress: null,
        newThreshold: null,
        validCount: null,
        policyFrom,
        policyTo,
        policyPassed: passedVal.value,
      };
    }

    // ── circuit-breaker state-change events ───────────────────────────────────
    //
    // Frozen / Unfrozen:
    //   topics: [Symbol("Frozen")] or [Symbol("Unfrozen")]
    //   data:   Void
    //
    // No address or payload — the event records that the named contract's
    // freeze state changed. The contract_id column identifies which breaker.

    if (eventType === "Frozen" || eventType === "Unfrozen") {
      return {
        ...base,
        signerAddress: null,
        newThreshold: null,
        validCount: null,
      };
    }

    return null;
  } catch (err) {
    console.error(`Failed to decode event from contract ${raw.contractId}:`, err);
    return null;
  }
}
