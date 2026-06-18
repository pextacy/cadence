import { describe, it, expect } from "vitest";
import { twap, evenSplit } from "./twap.js";
import { vwap, VWAP_PARTICIPATION_BPS } from "./vwap.js";
import { adaptive, ADAPTIVE_VOL_REFERENCE_BPS } from "./adaptive.js";
import { strategyFor, STRATEGIES } from "./registry.js";
import type { StrategyInput } from "./types.js";
import type { MarketSnapshot, RuntimeMandate } from "../../types.js";

function mandate(over: Partial<RuntimeMandate> = {}): RuntimeMandate {
  return {
    totalSell: 1_000_000n,
    endTimeMs: 0,
    maxSlippageBps: 100,
    priceFloor: 0n,
    priceCeiling: 0n,
    sellAsset: "CSPR",
    buyAsset: "USDC",
    venueAllowlist: ["cspr.trade"],
    strategy: "TWAP",
    ...over,
  };
}

function market(over: Partial<MarketSnapshot> = {}): MarketSnapshot {
  return { midPrice: 1_000_000_000n, takenAtMs: 0, ...over };
}

function input(over: Partial<StrategyInput> = {}): StrategyInput {
  return { remaining: 1_000_000n, slicesRemaining: 10, mandate: mandate(), market: market(), ...over };
}

describe("evenSplit", () => {
  it("splits remaining evenly across remaining slices", () => {
    expect(evenSplit(1_000_000n, 10)).toBe(100_000n);
  });
  it("returns the remainder when one slice is left", () => {
    expect(evenSplit(123_456n, 1)).toBe(123_456n);
  });
  it("guards against zero/negative slices", () => {
    expect(evenSplit(500n, 0)).toBe(500n);
  });
});

describe("twap strategy", () => {
  it("uses the even split and the mandate slippage cap", () => {
    const out = twap(input());
    expect(out.sliceSize).toBe(100_000n);
    expect(out.suggestedSlippageBps).toBe(100);
  });
});

describe("vwap strategy", () => {
  it("degrades to TWAP when depth is unknown", () => {
    expect(vwap(input()).sliceSize).toBe(100_000n);
  });
  it("caps the slice to the participation rate of observed depth", () => {
    // depth 200_000 * 20% = 40_000 < TWAP 100_000 → capped.
    const out = vwap(input({ market: market({ depthSell: 200_000n }) }));
    expect(out.sliceSize).toBe(40_000n);
    expect(VWAP_PARTICIPATION_BPS).toBe(2_000);
  });
  it("uses the full TWAP size when depth is ample", () => {
    const out = vwap(input({ market: market({ depthSell: 100_000_000n }) }));
    expect(out.sliceSize).toBe(100_000n);
  });
  it("degrades to TWAP when the participation cap rounds to zero", () => {
    expect(vwap(input({ market: market({ depthSell: 1n }) })).sliceSize).toBe(100_000n);
  });
});

describe("adaptive strategy", () => {
  it("uses the full TWAP size at or below the reference volatility", () => {
    const out = adaptive(input({ market: market({ volatilityBps: ADAPTIVE_VOL_REFERENCE_BPS }) }));
    expect(out.sliceSize).toBe(100_000n);
    expect(out.suggestedSlippageBps).toBe(100);
  });
  it("shrinks the slice and tightens slippage inversely to volatility", () => {
    // vol 1000, ref 500 → size 100_000*500/1000 = 50_000; slippage 100*500/1000 = 50.
    const out = adaptive(input({ market: market({ volatilityBps: 1_000 }) }));
    expect(out.sliceSize).toBe(50_000n);
    expect(out.suggestedSlippageBps).toBe(50);
  });
  it("floors the slice at 1/10 of TWAP and slippage at 1 under extreme volatility", () => {
    const out = adaptive(input({ market: market({ volatilityBps: 100_000 }) }));
    expect(out.sliceSize).toBe(10_000n);
    expect(out.suggestedSlippageBps).toBe(1);
  });
  it("degrades to TWAP when volatility is unknown", () => {
    expect(adaptive(input()).sliceSize).toBe(100_000n);
  });
});

describe("registry", () => {
  it("maps every strategy to an implementation", () => {
    expect(strategyFor("TWAP")).toBe(twap);
    expect(strategyFor("VWAP")).toBe(vwap);
    expect(strategyFor("ADAPTIVE")).toBe(adaptive);
    expect(Object.keys(STRATEGIES).sort()).toEqual(["ADAPTIVE", "TWAP", "VWAP"]);
  });
});
