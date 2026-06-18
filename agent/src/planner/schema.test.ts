import { describe, it, expect } from "vitest";
import { parseProposal, ProposalSchema } from "./schema.js";

describe("ProposalSchema", () => {
  it("parses a valid proposal into typed bigint", () => {
    const p = parseProposal({
      sellAmount: "100000",
      notBeforeMs: 1_700_000_000_000,
      maxSlippageBps: 100,
      reason: "TWAP slice 1 of 10",
    });
    expect(p.sellAmount).toBe(100_000n);
    expect(p.maxSlippageBps).toBe(100);
  });

  it("rejects a non-integer sellAmount", () => {
    expect(() => parseProposal({ sellAmount: "1.5", notBeforeMs: 0, maxSlippageBps: 10, reason: "x" })).toThrow();
  });

  it("rejects slippage above 100%", () => {
    expect(ProposalSchema.safeParse({ sellAmount: "1", notBeforeMs: 0, maxSlippageBps: 10_001, reason: "x" }).success).toBe(false);
  });

  it("rejects an empty reason", () => {
    expect(ProposalSchema.safeParse({ sellAmount: "1", notBeforeMs: 0, maxSlippageBps: 10, reason: "" }).success).toBe(false);
  });
});
