import { describe, it, expect } from "vitest";
import type { Mandate } from "@cadence/mandate";
import { toRuntimeMandate } from "./mandate.js";

function mandate(over: Partial<Mandate> = {}): Mandate {
  return {
    version: 1,
    treasury: "0x" + "11".repeat(20),
    sellAsset: "CSPR",
    buyAsset: "USDC",
    totalSellAmount: "2000000000000000",
    startTime: 1_751_328_000,
    endTime: 1_751_587_200,
    maxSlippageBps: 100,
    priceFloor: "0",
    priceCeiling: "0",
    strategy: "TWAP",
    venueAllowlist: ["cspr.trade"],
    nonce: "0x" + "00".repeat(32),
    ...over,
  };
}

describe("toRuntimeMandate", () => {
  it("carries the sell/buy assets through for per-mandate pairs", () => {
    const rt = toRuntimeMandate(mandate({ sellAsset: "WETH", buyAsset: "USDT" }));
    expect(rt.sellAsset).toBe("WETH");
    expect(rt.buyAsset).toBe("USDT");
  });

  it("converts the deadline from unix seconds to milliseconds", () => {
    const rt = toRuntimeMandate(mandate({ endTime: 1_700_000_000 }));
    expect(rt.endTimeMs).toBe(1_700_000_000_000);
  });

  it("parses amounts and prices as bigint and preserves the strategy", () => {
    const rt = toRuntimeMandate(mandate({ totalSellAmount: "12345", priceFloor: "7", strategy: "ADAPTIVE" }));
    expect(rt.totalSell).toBe(12_345n);
    expect(rt.priceFloor).toBe(7n);
    expect(rt.strategy).toBe("ADAPTIVE");
  });
});
