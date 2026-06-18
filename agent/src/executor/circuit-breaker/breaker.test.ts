import { describe, it, expect } from "vitest";
import {
  evaluateBreaker,
  DEFAULT_BREAKER_CONFIG,
  INITIAL_BREAKER,
  type BreakerConfig,
  type BreakerSnapshot,
} from "./breaker.js";

const cfg: BreakerConfig = {
  volatilityTripBps: 1_000,
  maxConsecutiveFailures: 3,
  volatilityResetBps: 500,
};
const open: BreakerSnapshot = { state: "open", consecutiveFailures: 0, reason: "test" };

describe("evaluateBreaker", () => {
  it("stays closed under normal volatility and a successful fill", () => {
    const s = evaluateBreaker(INITIAL_BREAKER, { volatilityBps: 200, lastOutcome: "filled" }, cfg);
    expect(s.state).toBe("closed");
    expect(s.consecutiveFailures).toBe(0);
  });

  it("trips open when volatility reaches the trip threshold", () => {
    const s = evaluateBreaker(INITIAL_BREAKER, { volatilityBps: 1_500 }, cfg);
    expect(s.state).toBe("open");
    expect(s.reason).toMatch(/volatility/i);
  });

  it("trips open after the configured consecutive non-fills", () => {
    let s = evaluateBreaker(INITIAL_BREAKER, { lastOutcome: "skipped" }, cfg);
    expect(s.state).toBe("closed");
    expect(s.consecutiveFailures).toBe(1);
    s = evaluateBreaker(s, { lastOutcome: "skipped" }, cfg);
    expect(s.consecutiveFailures).toBe(2);
    s = evaluateBreaker(s, { lastOutcome: "paused" }, cfg);
    expect(s.state).toBe("open");
    expect(s.consecutiveFailures).toBe(3);
    expect(s.reason).toMatch(/consecutive/i);
  });

  it("resets the failure streak on a fill", () => {
    const two: BreakerSnapshot = { state: "closed", consecutiveFailures: 2 };
    const s = evaluateBreaker(two, { lastOutcome: "filled" }, cfg);
    expect(s.consecutiveFailures).toBe(0);
    expect(s.state).toBe("closed");
  });

  it("stays open until volatility falls back to the reset threshold (hysteresis)", () => {
    const s = evaluateBreaker(open, { volatilityBps: 700 }, cfg);
    expect(s.state).toBe("open");
  });

  it("re-closes once volatility is calm and the streak is clear", () => {
    const s = evaluateBreaker(open, { volatilityBps: 400, lastOutcome: "filled" }, cfg);
    expect(s.state).toBe("closed");
  });

  it("ships a sane default config (reset below trip)", () => {
    expect(DEFAULT_BREAKER_CONFIG.volatilityResetBps).toBeLessThan(
      DEFAULT_BREAKER_CONFIG.volatilityTripBps,
    );
    expect(DEFAULT_BREAKER_CONFIG.maxConsecutiveFailures).toBeGreaterThan(0);
  });
});
