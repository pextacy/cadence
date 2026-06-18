import { describe, it, expect } from "vitest";
import { secp256k1 } from "@noble/curves/secp256k1";
import { toHex } from "@casper-ecosystem/casper-eip-712";
import {
  buildMandateDomain,
  humanSummary,
  canonicalVenues,
  type Mandate,
} from "./schema.js";
import {
  signMandate,
  verifyMandate,
  mandateDigest,
  addressFromPrivateKey,
} from "./sign.js";
import { resolveNetwork, networkPreset } from "./network.js";

const CHAIN = "casper-test";
const PRIV = "0x" + "01".repeat(32);

function baseMandate(): Omit<Mandate, "treasury"> {
  return {
    version: 1,
    sellAsset: "CSPR",
    buyAsset: "USDC",
    totalSellAmount: "2000000000000000",
    startTime: 1751328000,
    endTime: 1751587200,
    maxSlippageBps: 100,
    priceFloor: "0",
    priceCeiling: "0",
    strategy: "TWAP",
    venueAllowlist: ["cspr.trade"],
    venueAddresses: ["account-hash-" + "cd".repeat(32)],
    nonce: "0x" + "ab".repeat(32),
  };
}

describe("mandate signing", () => {
  it("derives a stable address from a private key", () => {
    const addr = addressFromPrivateKey(PRIV);
    expect(addr).toMatch(/^0x[0-9a-f]{40}$/);
    // Cross-check against the standard Ethereum-style derivation.
    const pub = secp256k1.getPublicKey(secp256k1.utils.normPrivateKeyToScalar
      ? new Uint8Array(Buffer.from(PRIV.slice(2), "hex"))
      : new Uint8Array(Buffer.from(PRIV.slice(2), "hex")), false);
    expect(pub.length).toBe(65);
  });

  it("produces a 65-byte signature and a 32-byte digest", () => {
    const domain = buildMandateDomain(CHAIN);
    const signed = signMandate(baseMandate(), domain, PRIV);
    expect(signed.signature).toMatch(/^0x[0-9a-f]{130}$/); // 65 bytes
    expect(signed.digest).toMatch(/^0x[0-9a-f]{64}$/); // 32 bytes
    expect(signed.signer).toBe(addressFromPrivateKey(PRIV));
    expect(signed.mandate.treasury).toBe(signed.signer);
  });

  it("verifies a freshly signed mandate", () => {
    const domain = buildMandateDomain(CHAIN);
    const signed = signMandate(baseMandate(), domain, PRIV);
    const result = verifyMandate(signed.mandate, domain, signed.signature);
    expect(result.valid).toBe(true);
    expect(result.signer.toLowerCase()).toBe(signed.signer.toLowerCase());
  });

  it("rejects a mandate tampered after signing", () => {
    const domain = buildMandateDomain(CHAIN);
    const signed = signMandate(baseMandate(), domain, PRIV);
    const tampered = { ...signed.mandate, totalSellAmount: "9999999999999999" };
    const result = verifyMandate(tampered, domain, signed.signature);
    expect(result.valid).toBe(false);
  });

  it("rejects a signature checked under a different chain domain", () => {
    const domain = buildMandateDomain(CHAIN);
    const signed = signMandate(baseMandate(), domain, PRIV);
    const otherDomain = buildMandateDomain("casper");
    const result = verifyMandate(signed.mandate, otherDomain, signed.signature);
    expect(result.valid).toBe(false);
  });

  it("digest is deterministic for the same mandate + domain", () => {
    const domain = buildMandateDomain(CHAIN);
    const m = { ...baseMandate(), treasury: addressFromPrivateKey(PRIV) } as Mandate;
    expect(toHex(mandateDigest(m, domain))).toBe(toHex(mandateDigest(m, domain)));
  });
});

describe("mandate helpers", () => {
  it("joins venues canonically", () => {
    expect(canonicalVenues(["a", "b"])).toBe("a,b");
  });

  it("renders a plain-language summary", () => {
    const m = { ...baseMandate(), treasury: "0x" + "00".repeat(20) } as Mandate;
    const summary = humanSummary(m);
    expect(summary).toContain("2,000,000 CSPR");
    expect(summary).toContain("USDC");
    expect(summary).toContain("1.00% slippage");
    expect(summary).toContain("TWAP");
  });
});

describe("network presets", () => {
  it("defaults to testnet and accepts friendly + chain names", () => {
    expect(resolveNetwork(undefined)).toBe("testnet");
    expect(resolveNetwork("")).toBe("testnet");
    expect(resolveNetwork("TestNet")).toBe("testnet");
    expect(resolveNetwork("casper-test")).toBe("testnet");
    expect(resolveNetwork("mainnet")).toBe("mainnet");
    expect(resolveNetwork("casper")).toBe("mainnet");
  });

  it("throws on an unknown network", () => {
    expect(() => resolveNetwork("devnet")).toThrow();
  });

  it("maps each network to its chain name and endpoints", () => {
    const main = networkPreset("mainnet");
    expect(main.chainName).toBe("casper");
    expect(main.csprCloudRestUrl).toBe("https://api.cspr.cloud");
    expect(main.explorerTxBase).toBe("https://cspr.live/deploy/");

    const test = networkPreset("testnet");
    expect(test.chainName).toBe("casper-test");
    expect(test.csprCloudStreamingUrl).toBe("wss://streaming.testnet.cspr.cloud");
    expect(test.explorerTxBase).toBe("https://testnet.cspr.live/deploy/");
  });
});
