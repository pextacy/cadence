import { describe, expect, it } from "vitest";
import {
  buildMandatePreimage,
  bytesToHexPure,
  hexToBytesPure,
  parseCasperAddress,
  type CasperAddress,
} from "./casperAuth.js";

/**
 * The exact byte layout the contract pins in `contracts/vault/tests/signature.rs`
 * (`GOLDEN_PREIMAGE_HEX`). If this assertion fails, the TS preimage builder has
 * drifted from `ExecutionVault::mandate_message` and every signed mandate would
 * fail on-chain verification. Keep both golden vectors identical.
 */
const GOLDEN_PREIMAGE_HEX =
  "436164656e63652d4d616e646174652d763100105b69f2d74a211a6cb337cba6751a8f15cc7b44b7c65329c29731b67e1ac047000ee624f1bbaec23e6dc6b877815f322058045c45cdbf8ef53e7db2c58b0af309040000004353505204000000555344430340420f40420f0000000000640000000000010000000a000000637370722e74726164650100000000147f2cc33b4fdb04ab4e9ef2c067137177097ba50a544a0a343ce636028fcfcf200000000505050505050505050505050505050505050505050505050505050505050505";

const account = (hex: string): CasperAddress => ({ tag: 0, hash: hexToBytesPure(hex) });

/** The three 32-byte account hashes are sliced verbatim out of the golden vector
 * at their known byte offsets, so the test exercises the framing/field-order of the
 * encoder (the part that can drift) without any error-prone hand-transcription. */
const goldenAccount = (byteStart: number): CasperAddress =>
  account(GOLDEN_PREIMAGE_HEX.slice(byteStart * 2, (byteStart + 32) * 2));

describe("Casper mandate preimage", () => {
  it("reproduces the frozen golden vector byte-for-byte", () => {
    // The fixed inputs the Rust golden vector captures (odra_test accounts 0/1/2).
    const preimage = buildMandatePreimage({
      agent: goldenAccount(19),
      treasury: goldenAccount(52),
      sellAsset: "CSPR",
      buyAsset: "USDC",
      totalSell: 1_000_000n,
      endTimeMs: 1_000_000n,
      maxSlippageBps: 100,
      priceFloor: 0n,
      priceCeiling: 0n,
      venues: ["cspr.trade"],
      venueAddresses: [goldenAccount(141)],
      nonce: new Uint8Array(32).fill(5),
    });
    expect(bytesToHexPure(preimage)).toBe(GOLDEN_PREIMAGE_HEX);
  });

  it("parses account-hash addresses as tag 0", () => {
    const a = parseCasperAddress(
      "account-hash-105b69f2d74a211a6cb337cba6751a8f15cc7b44b7c65329c29731b67e1ac047",
    );
    expect(a.tag).toBe(0);
    expect(a.hash).toHaveLength(32);
  });

  it("parses contract hash addresses as tag 1", () => {
    const a = parseCasperAddress(`hash-${"aa".repeat(32)}`);
    expect(a.tag).toBe(1);
    expect(a.hash).toHaveLength(32);
  });

  it("rejects an unrecognised address", () => {
    expect(() => parseCasperAddress("not-an-address")).toThrow();
  });

  it("encodes a non-zero price band into the preimage length", () => {
    const base = {
      agent: account("00".repeat(32)),
      treasury: account("11".repeat(32)),
      sellAsset: "CSPR",
      buyAsset: "USDC",
      totalSell: 1_000_000n,
      endTimeMs: 1_000_000n,
      maxSlippageBps: 100,
      venues: ["cspr.trade"],
      venueAddresses: [account("22".repeat(32))],
      nonce: new Uint8Array(32).fill(9),
    };
    const noBand = buildMandatePreimage({ ...base, priceFloor: 0n, priceCeiling: 0n });
    const withBand = buildMandatePreimage({
      ...base,
      priceFloor: 1_500_000_000n,
      priceCeiling: 2_500_000_000n,
    });
    // A set band adds magnitude bytes to both U512 fields, so the preimage grows.
    expect(withBand.length).toBeGreaterThan(noBand.length);
  });
});
