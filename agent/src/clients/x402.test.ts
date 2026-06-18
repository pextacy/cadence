import { describe, it, expect } from "vitest";
import {
  buildPaymentPayload,
  encodePaymentHeader,
  recoverPaymentSigner,
  selectCasperRequirement,
  type PaymentRequirements,
  type Payment402Body,
} from "./x402.js";

const REQ: PaymentRequirements = {
  scheme: "exact",
  network: "casper:casper-test",
  payTo: "0x" + "ab".repeat(32),
  amount: "1000000",
  asset: "0x" + "cd".repeat(32),
  maxTimeoutSeconds: 120,
  extra: { name: "Cep18x402", version: "1" },
};

const PRIV = "0x" + "02".repeat(32);
const FROM = "0x" + "11".repeat(32);

describe("selectCasperRequirement", () => {
  it("picks the matching network", () => {
    const body: Payment402Body = { x402Version: 2, accepts: [REQ] };
    expect(selectCasperRequirement(body, "casper:casper-test")).toBe(REQ);
  });
  it("throws when no option matches", () => {
    const body: Payment402Body = { x402Version: 2, accepts: [REQ] };
    expect(() => selectCasperRequirement(body, "casper:casper")).toThrow();
  });
});

describe("buildPaymentPayload", () => {
  const payload = buildPaymentPayload(REQ, {
    resourceUrl: "https://x402.example/api/v1/market-depth",
    from: FROM,
    nonce: "0x" + "99".repeat(32),
    nowSec: 1_700_000_000,
    privateKeyHex: PRIV,
  });

  it("produces a 65-byte signature and mirrors the requirement", () => {
    expect(payload.payload.signature).toMatch(/^0x[0-9a-f]{130}$/);
    expect(payload.accepted).toEqual(REQ);
    expect(payload.payload.authorization.to).toBe(REQ.payTo);
    expect(payload.payload.authorization.value).toBe(REQ.amount);
    expect(payload.payload.authorization.valid_before).toBe(1_700_000_000 + 120);
  });

  it("signature recovers a stable signer (round-trip)", () => {
    const signer = recoverPaymentSigner(payload);
    expect(signer).toMatch(/^0x[0-9a-f]{40}$/);
    // Recovering again yields the same address.
    expect(recoverPaymentSigner(payload)).toBe(signer);
  });

  it("header encodes to base64 JSON", () => {
    const header = encodePaymentHeader(payload);
    const decoded = JSON.parse(Buffer.from(header, "base64").toString("utf8"));
    expect(decoded.x402Version).toBe(2);
    expect(decoded.accepted.network).toBe("casper:casper-test");
  });
});
