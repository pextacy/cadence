import { describe, it, expect } from "vitest";
import { computeMinOut, impliedSlippageBps, priceFixed, withinBand, PRICE_SCALE } from "./units.js";

describe("computeMinOut", () => {
  it("applies the slippage cap", () => {
    expect(computeMinOut(200_000n, 100)).toBe(198_000n); // 1%
    expect(computeMinOut(200_000n, 0)).toBe(200_000n);
  });
  it("rejects out-of-range slippage", () => {
    expect(() => computeMinOut(200_000n, 10_001)).toThrow();
    expect(() => computeMinOut(0n, 100)).toThrow();
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
