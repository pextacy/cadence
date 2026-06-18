import { describe, it, expect } from "vitest";
import { Portfolio } from "./manager.js";
import type { MandateTrack } from "./types.js";
import type { RuntimeMandate, VaultState, VaultStatus } from "../types.js";

const NOW = 1_000_000;

function mandate(over: Partial<RuntimeMandate> = {}): RuntimeMandate {
  return {
    totalSell: 1_000_000n,
    endTimeMs: NOW + 100_000,
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

function state(over: Partial<VaultState> = {}): VaultState {
  return { status: "Active", soldSoFar: 0n, boughtSoFar: 0n, sliceCount: 0, totalSell: 1_000_000n, ...over };
}

function track(id: string): MandateTrack {
  return { id, mandate: mandate(), state: state() };
}

describe("Portfolio", () => {
  it("lists and looks up tracks by id", () => {
    const p = new Portfolio([track("a"), track("b")]);
    expect(p.list().map((t) => t.id)).toEqual(["a", "b"]);
    expect(p.get("b")?.id).toBe("b");
    expect(p.get("missing")).toBeUndefined();
  });

  it("returns a new Portfolio when a track state is replaced (immutability)", () => {
    const original = new Portfolio([track("a"), track("b")]);
    const next = original.withTrackState("a", state({ soldSoFar: 500_000n, sliceCount: 1 }));
    expect(next).not.toBe(original);
    expect(next.get("a")?.state.soldSoFar).toBe(500_000n);
    // Original is unchanged.
    expect(original.get("a")?.state.soldSoFar).toBe(0n);
  });

  it("throws when updating an unknown track", () => {
    const p = new Portfolio([track("a")]);
    expect(() => p.withTrackState("nope", state())).toThrow();
  });

  it("selects the next actionable track via the scheduler", () => {
    const p = new Portfolio([track("a"), track("b")]);
    expect(p.selectNext(NOW)).not.toBeNull();
  });

  it("reports done when no track is actionable", () => {
    const done = new Portfolio([track("a")]).withTrackState("a", state({ soldSoFar: 1_000_000n }));
    expect(done.allDone(NOW)).toBe(true);
    expect(done.selectNext(NOW)).toBeNull();
  });

  it("is not done while a track remains actionable", () => {
    expect(new Portfolio([track("a")]).allDone(NOW)).toBe(false);
  });
});
