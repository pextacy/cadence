import { describe, it, expect } from "vitest";
import { PRICE_SCALE } from "@cadence/mandate";
import { reduceEvents, deriveMetrics, initialState } from "./events.js";
import type { VaultEvent } from "../types.js";

const NOW = 1_000_000;

const seq: VaultEvent[] = [
  { kind: "MandateInitialised", treasury: "0xt", agent: "0xa", sellAsset: "CSPR", buyAsset: "USDC", totalSell: "1000000", endTimeMs: NOW + 60_000, maxSlippageBps: 100 },
  { kind: "VaultFunded", amount: "1000000", balance: "1000000" },
  { kind: "SliceExecuted", sliceId: 0, sellAmount: "100000", quotedOut: "200000", minOut: "198000", venue: "cspr.trade", soldSoFar: "100000", deployHash: "0xdeploy0" },
  { kind: "FillRecorded", sliceId: 0, boughtAmount: "199000", swapDeployHash: "0xswap0", boughtSoFar: "199000" },
  { kind: "DecisionAttested", sliceId: 0, reason: "TWAP slice 1 of 10" },
];

describe("reduceEvents", () => {
  it("reconstructs state from the event stream", () => {
    const s = reduceEvents(seq);
    expect(s.status).toBe("Active");
    expect(s.totalSell).toBe(1_000_000n);
    expect(s.soldSoFar).toBe(100_000n);
    expect(s.boughtSoFar).toBe(199_000n);
    expect(s.slices).toHaveLength(1);
    expect(s.slices[0]).toMatchObject({ status: "filled", reason: "TWAP slice 1 of 10", venue: "cspr.trade" });
  });

  it("reflects pause and settlement", () => {
    const paused = reduceEvents([...seq, { kind: "StatusChanged", paused: true }]);
    expect(paused.status).toBe("Paused");
    const settled = reduceEvents([
      ...seq,
      { kind: "Settled", completed: true, soldSoFar: "1000000", boughtSoFar: "1990000", sliceCount: 10, returnedToTreasury: "0" },
    ]);
    expect(settled.status).toBe("Completed");
    expect(settled.settled?.sliceCount).toBe(10);
  });

  it("reflects an emergency withdrawal as the terminal Halted state", () => {
    const halted = reduceEvents([
      ...seq,
      { kind: "StatusChanged", paused: true },
      { kind: "EmergencyWithdrawn", returnedToTreasury: "801000", soldSoFar: "100000" },
    ]);
    expect(halted.status).toBe("Halted");
    expect(halted.settled?.completed).toBe(false);
    expect(halted.settled?.returnedToTreasury).toBe(801_000n);
  });
});

describe("deriveMetrics", () => {
  it("computes remaining, average price and time left", () => {
    const m = deriveMetrics(reduceEvents(seq), NOW, null);
    expect(m.remaining).toBe(900_000n);
    expect(m.averagePrice).toBe((199_000n * PRICE_SCALE) / 100_000n);
    expect(m.timeLeftMs).toBe(60_000);
    expect(m.slippageSavedBps).toBeNull();
  });

  it("computes slippage saved vs a naive baseline", () => {
    const state = reduceEvents(seq);
    // realised avg = 1.99; naive baseline = 1.95 → ~205 bps better
    const naive = (195n * PRICE_SCALE) / 100n;
    const m = deriveMetrics(state, NOW, naive);
    expect(m.slippageSavedBps).not.toBeNull();
    expect(m.slippageSavedBps!).toBeGreaterThan(0);
  });

  it("returns honest nulls on empty state", () => {
    const m = deriveMetrics(initialState(), NOW, null);
    expect(m.averagePrice).toBeNull();
    expect(m.timeLeftMs).toBeNull();
  });
});
