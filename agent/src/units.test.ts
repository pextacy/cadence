import { describe, it, expect } from "vitest";
import {
  computeMinOut,
  impliedSlippageBps,
  priceFixed,
  withinBand,
  withinSlippage,
  PRICE_SCALE,
} from "./units.js";

describe("computeMinOut", () => {
  it("applies the slippage cap", () => {
    expect(computeMinOut(200_000n, 100)).toBe(198_000n); // 1%
    expect(computeMinOut(200_000n, 0)).toBe(200_000n);
  });
  it("rejects out-of-range slippage", () => {
    expect(() => computeMinOut(200_000n, 10_001)).toThrow();
    expect(() => computeMinOut(0n, 100)).toThrow();
  });
  it("rounds up so min_out always passes the vault's cross-multiply check", () => {
    // Regression: at quotedOut=20001, bps=100 a floored min_out (19800) has
    // implied slippage that the contract's predicate rejects. Rounding up (19801)
    // must satisfy withinSlippage for the same cap.
    const q = 20_001n;
    const minOut = computeMinOut(q, 100);
    expect(minOut).toBe(19_801n);
    expect(withinSlippage(q, minOut, 100)).toBe(true);
  });
});

describe("withinSlippage", () => {
  it("mirrors the contract predicate (q - m) * BPS <= q * bps", () => {
    // The floored value the old code produced is correctly REJECTED here.
    expect(withinSlippage(20_001n, 19_800n, 100)).toBe(false);
    expect(withinSlippage(20_001n, 19_801n, 100)).toBe(true);
    expect(withinSlippage(200_000n, 198_000n, 100)).toBe(true);
    expect(withinSlippage(200_000n, 200_000n, 0)).toBe(true);
  });
  it("rejects a min_out above the quote", () => {
    expect(withinSlippage(100n, 101n, 100)).toBe(false);
  });
});

describe("impliedSlippageBps", () => {
  it("matches the contract's integer formula", () => {
    expect(impliedSlippageBps(200_000n, 198_000n)).toBe(100);
    expect(impliedSlippageBps(200_000n, 197_000n)).toBe(150);
    expect(impliedSlippageBps(200_000n, 200_000n)).toBe(0);
  });
});

describe("priceFixed", () => {
  it("computes buy-units per sell-unit in fixed point", () => {
    expect(priceFixed(200_000n, 100_000n)).toBe(2n * PRICE_SCALE);
  });
  it("rejects zero sell amount", () => {
    expect(() => priceFixed(1n, 0n)).toThrow();
  });
});

describe("withinBand", () => {
  it("treats 0 bounds as unset", () => {
    expect(withinBand(5n, 0n, 0n)).toBe(true);
  });
  it("enforces floor and ceiling", () => {
    expect(withinBand(2n * PRICE_SCALE, PRICE_SCALE + 500_000_000n, 3n * PRICE_SCALE)).toBe(true);
    expect(withinBand(PRICE_SCALE, 3n * PRICE_SCALE / 2n, 0n)).toBe(false);
    expect(withinBand(3n * PRICE_SCALE, 0n, 5n * PRICE_SCALE / 2n)).toBe(false);
  });
});
