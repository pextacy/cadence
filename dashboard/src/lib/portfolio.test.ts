import { describe, it, expect } from "vitest";
import { aggregatePortfolio } from "./portfolio.js";
import type { DashboardState, VaultStatus } from "../types.js";

function st(over: Partial<DashboardState> = {}): DashboardState {
  return { status: "Active", totalSell: 1_000n, soldSoFar: 0n, boughtSoFar: 0n, slices: [], ...over };
}

const NOW = 1_000_000;

describe("aggregatePortfolio", () => {
  it("is all-zero / null for an empty portfolio", () => {
    const s = aggregatePortfolio([], NOW);
    expect(s.mandateCount).toBe(0);
    expect(s.totalRemaining).toBe(0n);
    expect(s.totalSold).toBe(0n);
    expect(s.averagePrice).toBeNull();
    expect(s.nearestDeadlineMs).toBeNull();
  });

  it("counts mandates by status", () => {
    const states = [
      st({ status: "Active" }),
      st({ status: "Paused" }),
      st({ status: "Completed" }),
      st({ status: "Expired" }),
    ];
    const s = aggregatePortfolio(states, NOW);
    expect(s.mandateCount).toBe(4);
    expect(s.activeCount).toBe(1);
    expect(s.pausedCount).toBe(1);
    expect(s.completedCount).toBe(2); // Completed + Expired
  });

  it("sums remaining, clamping each track at zero", () => {
    const states = [
      st({ totalSell: 1_000n, soldSoFar: 400n }), // 600 remaining
      st({ totalSell: 1_000n, soldSoFar: 1_000n }), // 0 remaining
    ];
    expect(aggregatePortfolio(states, NOW).totalRemaining).toBe(600n);
  });

  it("computes the aggregate realised average price across tracks", () => {
    // total sold 1000, total bought 2000 → 2.0 in PRICE_SCALE (1e9).
    const states = [st({ soldSoFar: 600n, boughtSoFar: 1_200n }), st({ soldSoFar: 400n, boughtSoFar: 800n })];
    const s = aggregatePortfolio(states, NOW);
    expect(s.totalSold).toBe(1_000n);
    expect(s.totalBought).toBe(2_000n);
    expect(s.averagePrice).toBe(2_000_000_000n);
  });

  it("reports the nearest deadline among non-settled tracks", () => {
    const states = [
      st({ status: "Active", endTimeMs: NOW + 50_000 }),
      st({ status: "Active", endTimeMs: NOW + 10_000 }),
      st({ status: "Completed", endTimeMs: NOW + 1 }), // settled: ignored
    ];
    expect(aggregatePortfolio(states, NOW).nearestDeadlineMs).toBe(NOW + 10_000);
  });

  it("ignores tracks with unknown status in counts but still in mandateCount", () => {
    const s = aggregatePortfolio([st({ status: "Unknown" as VaultStatus })], NOW);
    expect(s.mandateCount).toBe(1);
    expect(s.activeCount).toBe(0);
  });
});
