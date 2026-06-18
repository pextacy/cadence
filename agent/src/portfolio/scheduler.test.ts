import { describe, it, expect } from "vitest";
import { isActionable, selectNext } from "./scheduler.js";
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

function track(id: string, m: Partial<RuntimeMandate> = {}, s: Partial<VaultState> = {}): MandateTrack {
  return { id, mandate: mandate(m), state: state(s) };
}

describe("isActionable", () => {
  it("is true for an active, incomplete, in-window track", () => {
    expect(isActionable(track("a"), NOW)).toBe(true);
  });
  it("is false when paused", () => {
    expect(isActionable(track("a", {}, { status: "Paused" as VaultStatus }), NOW)).toBe(false);
  });
  it("is false when the order is complete", () => {
    expect(isActionable(track("a", {}, { soldSoFar: 1_000_000n }), NOW)).toBe(false);
  });
  it("is false when the window has closed", () => {
    expect(isActionable(track("a", { endTimeMs: NOW - 1 }), NOW)).toBe(false);
  });
});

describe("selectNext", () => {
  it("returns null when there are no tracks", () => {
    expect(selectNext({ tracks: [], nowMs: NOW })).toBeNull();
  });
  it("returns null when no track is actionable", () => {
    const tracks = [track("a", {}, { status: "Paused" as VaultStatus }), track("b", {}, { soldSoFar: 1_000_000n })];
    expect(selectNext({ tracks, nowMs: NOW })).toBeNull();
  });
  it("picks the track with the nearer deadline when remaining is equal", () => {
    const soon = track("soon", { endTimeMs: NOW + 10_000 });
    const later = track("later", { endTimeMs: NOW + 100_000 });
    expect(selectNext({ tracks: [later, soon], nowMs: NOW })?.id).toBe("soon");
  });
  it("picks the track with more remaining when deadlines are equal", () => {
    const big = track("big", {}, { soldSoFar: 100_000n });
    const small = track("small", {}, { soldSoFar: 900_000n });
    expect(selectNext({ tracks: [small, big], nowMs: NOW })?.id).toBe("big");
  });
  it("ignores paused and completed tracks", () => {
    const paused = track("paused", { endTimeMs: NOW + 1 }, { status: "Paused" as VaultStatus });
    const live = track("live", { endTimeMs: NOW + 100_000 });
    expect(selectNext({ tracks: [paused, live], nowMs: NOW })?.id).toBe("live");
  });
  it("is deterministic for fully-equal tracks (lexicographic id tie-break)", () => {
    const a = track("aaa");
    const b = track("bbb");
    expect(selectNext({ tracks: [b, a], nowMs: NOW })?.id).toBe("aaa");
    expect(selectNext({ tracks: [a, b], nowMs: NOW })?.id).toBe("aaa");
  });
});
