/**
 * The canonical Casper-native mandate preimage — **FROZEN**.
 *
 * This is the byte-for-byte TypeScript mirror of `ExecutionVault::mandate_message`
 * (`contracts/vault/src/vault/preimage.rs`). The treasury signs these exact bytes
 * with their Casper key off-chain; the vault reconstructs the same bytes at `init`
 * and verifies the signature with `env().verify_signature`. ANY divergence in field
 * order, framing, or the domain tag makes every signed mandate fail on-chain.
 *
 * Field order (frozen):
 *   domain_tag ‖ agent ‖ treasury ‖ sell_asset ‖ buy_asset ‖ total_sell ‖
 *   end_time_ms ‖ max_slippage_bps ‖ price_floor ‖ price_ceiling ‖ venues ‖
 *   venue_addresses ‖ nonce
 *
 * Encodings are Casper `ToBytes`: u32/u64 little-endian, U512 = 1 length byte +
 * little-endian magnitude, String/Bytes = u32 length + raw, Vec<T> = u32 count +
 * items, Address = 1 tag byte (0 account / 1 contract) + 32-byte hash. The
 * `preimage_golden_vector_is_frozen` Rust test and `casperAuth.test.ts` pin the
 * exact bytes so accidental drift fails the build on both sides. Pure data — no
 * casper-js-sdk dependency, so it is safe to import in the browser.
 */

const TEXT = new TextEncoder();

/** Frozen domain prefix, raw ASCII with no length framing (matches MANDATE_DOMAIN_TAG). */
export const MANDATE_DOMAIN_TAG = "Cadence-Mandate-v1";

/** Casper `Address` tag: account hash (0) or contract/package hash (1). */
export type CasperAddressTag = 0 | 1;

/** A decoded Casper address: its kind tag plus the raw 32-byte hash. */
export interface CasperAddress {
  tag: CasperAddressTag;
  hash: Uint8Array;
}

function u32le(n: number): Uint8Array {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, n >>> 0, true);
  return out;
}

function u64le(n: bigint): Uint8Array {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt.asUintN(64, n), true);
  return out;
}

/** Casper U512 `ToBytes`: a length byte then the little-endian magnitude (no leading zeros). */
function u512(n: bigint): Uint8Array {
  if (n < 0n) throw new Error("U512 cannot encode a negative value");
  if (n === 0n) return new Uint8Array([0]);
  const le: number[] = [];
  let v = n;
  while (v > 0n) {
    le.push(Number(v & 0xffn));
    v >>= 8n;
  }
  if (le.length > 64) throw new Error("value exceeds U512");
  return new Uint8Array([le.length, ...le]);
}

/** Casper `String`/`Bytes` `ToBytes`: u32 length prefix then the raw bytes. */
function lenPrefixed(bytes: Uint8Array): Uint8Array {
  return concatBytes([u32le(bytes.length), bytes]);
}

function clString(s: string): Uint8Array {
  return lenPrefixed(TEXT.encode(s));
}

function clAddress(a: CasperAddress): Uint8Array {
  if (a.hash.length !== 32) {
    throw new Error(`Casper address hash must be 32 bytes, got ${a.hash.length}`);
  }
  return concatBytes([new Uint8Array([a.tag]), a.hash]);
}

function vecStrings(items: string[]): Uint8Array {
  return concatBytes([u32le(items.length), ...items.map(clString)]);
}

function vecAddresses(items: CasperAddress[]): Uint8Array {
  return concatBytes([u32le(items.length), ...items.map(clAddress)]);
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

/** Parse a `0x?`-prefixed or bare hex string into bytes. */
export function hexToBytesPure(hex: string): Uint8Array {
  const clean = hex.replace(/^0x/, "");
  if (clean.length % 2 !== 0) throw new Error(`odd-length hex: ${hex}`);
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** Lowercase hex (no prefix) for a byte array. */
export function bytesToHexPure(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}

/**
 * Decode a Casper address string into the tag + 32-byte hash the preimage uses.
 * `account-hash-…` → account (tag 0); `hash-…` / `contract-…` / `contract-package-…`
 * → contract (tag 1). A bare 64-char hex string is treated as an account hash.
 */
export function parseCasperAddress(addr: string): CasperAddress {
  const s = addr.trim();
  const account = s.match(/^account-hash-([0-9a-fA-F]{64})$/);
  if (account) return { tag: 0, hash: hexToBytesPure(account[1]!) };
  const contract = s.match(/^(?:contract-package-|contract-|hash-)([0-9a-fA-F]{64})$/);
  if (contract) return { tag: 1, hash: hexToBytesPure(contract[1]!) };
  const bare = s.match(/^(?:0x)?([0-9a-fA-F]{64})$/);
  if (bare) return { tag: 0, hash: hexToBytesPure(bare[1]!) };
  throw new Error(
    `Unrecognised Casper address "${addr}" (expected account-hash-… / hash-… / 64-hex)`,
  );
}

/** Inputs to {@link buildMandatePreimage}, all already in canonical form. */
export interface MandatePreimageParams {
  agent: CasperAddress;
  treasury: CasperAddress;
  sellAsset: string;
  buyAsset: string;
  totalSell: bigint;
  endTimeMs: bigint;
  maxSlippageBps: number;
  priceFloor: bigint;
  priceCeiling: bigint;
  venues: string[];
  venueAddresses: CasperAddress[];
  /** The mandate nonce, raw bytes (typically 32). */
  nonce: Uint8Array;
}

/**
 * Build the frozen Casper-native mandate preimage. Must stay byte-for-byte
 * identical to `ExecutionVault::mandate_message`.
 */
export function buildMandatePreimage(p: MandatePreimageParams): Uint8Array {
  return concatBytes([
    TEXT.encode(MANDATE_DOMAIN_TAG),
    clAddress(p.agent),
    clAddress(p.treasury),
    clString(p.sellAsset),
    clString(p.buyAsset),
    u512(p.totalSell),
    u64le(p.endTimeMs),
    u32le(p.maxSlippageBps),
    u512(p.priceFloor),
    u512(p.priceCeiling),
    vecStrings(p.venues),
    vecAddresses(p.venueAddresses),
    lenPrefixed(p.nonce),
  ]);
}
