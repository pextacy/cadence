import { describe, it, expect } from "vitest";
import { realisedVolatilityBps } from "./volatility.js";

describe("realisedVolatilityBps", () => {
  it("returns undefined for fewer than two prices", () => {
    expect(realisedVolatilityBps([])).toBeUndefined();
    expect(realisedVolatilityBps([100n])).toBeUndefined();
  });

  it("returns zero for a flat price series", () => {
    expect(realisedVolatilityBps([100n, 100n, 100n])).toBe(0);
  });

  it("returns a positive integer bps figure for a varying series", () => {
    const vol = realisedVolatilityBps([100n, 110n, 90n, 120n]);
    expect(vol).toBeGreaterThan(0);
    expect(Number.isInteger(vol)).toBe(true);
  });

  it("is deterministic", () => {
    const series = [1_000n, 1_010n, 995n, 1_020n, 1_005n];
    expect(realisedVolatilityBps(series)).toBe(realisedVolatilityBps(series));
  });
});
