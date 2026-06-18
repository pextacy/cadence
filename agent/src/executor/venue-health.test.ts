import { describe, it, expect } from "vitest";
import {
  VenueHealthTracker,
  DEFAULT_VENUE_HEALTH_CONFIG,
  type VenueHealthConfig,
} from "./venue-health.js";

const cfg: VenueHealthConfig = {
  maxConsecutiveFailures: 3,
  cooldownMs: 60_000,
  latencyFailMs: 10_000,
};

const T0 = 1_000_000;

describe("VenueHealthTracker", () => {
  it("treats an unknown venue as healthy", () => {
    const t = VenueHealthTracker.empty(cfg);
    expect(t.isHealthy("a", T0)).toBe(true);
    expect(t.healthy(["a", "b"], T0)).toEqual(["a", "b"]);
  });

  it("is immutable — record returns a new tracker", () => {
    const t0 = VenueHealthTracker.empty(cfg);
    const t1 = t0.record("a", "fail", 50, T0);
    expect(t1).not.toBe(t0);
    expect(t0.snapshot()).toEqual({});
  });

  it("quarantines a venue after the configured consecutive failures", () => {
    let t = VenueHealthTracker.empty(cfg);
    t = t.record("a", "fail", 50, T0);
    expect(t.isHealthy("a", T0)).toBe(true);
    t = t.record("a", "fail", 50, T0);
    expect(t.snapshot().a?.failures).toBe(2);
    t = t.record("a", "fail", 50, T0);
    expect(t.isHealthy("a", T0)).toBe(false);
    expect(t.snapshot().a?.state).toBe("cooling");
  });

  it("ejects only the failing venue from the allowlist, not the whole mandate", () => {
    let t = VenueHealthTracker.empty(cfg);
    for (let i = 0; i < 3; i++) t = t.record("bad", "fail", 50, T0);
    expect(t.healthy(["good", "bad", "other"], T0)).toEqual(["good", "other"]);
  });

  it("returns the venue to routable after the cooldown elapses", () => {
    let t = VenueHealthTracker.empty(cfg);
    for (let i = 0; i < 3; i++) t = t.record("a", "fail", 50, T0);
    expect(t.isHealthy("a", T0 + 59_999)).toBe(false);
    expect(t.isHealthy("a", T0 + 60_000)).toBe(true);
  });

  it("heals fully on a clean fast success", () => {
    let t = VenueHealthTracker.empty(cfg);
    t = t.record("a", "fail", 50, T0);
    t = t.record("a", "fail", 50, T0);
    t = t.record("a", "ok", 50, T0);
    expect(t.snapshot().a?.failures).toBe(0);
    expect(t.snapshot().a?.state).toBe("up");
  });

  it("treats a slow success as a soft failure", () => {
    let t = VenueHealthTracker.empty(cfg);
    for (let i = 0; i < 3; i++) t = t.record("a", "ok", 12_000, T0);
    expect(t.isHealthy("a", T0)).toBe(false);
  });

  it("re-quarantines on the next failure after probation", () => {
    let t = VenueHealthTracker.empty(cfg);
    for (let i = 0; i < 3; i++) t = t.record("a", "fail", 50, T0);
    const probe = T0 + 60_000;
    expect(t.isHealthy("a", probe)).toBe(true);
    // failure count is still at the cap, so a single further fail re-cools it
    t = t.record("a", "fail", 50, probe);
    expect(t.isHealthy("a", probe)).toBe(false);
  });

  it("ships a sane default config", () => {
    expect(DEFAULT_VENUE_HEALTH_CONFIG.maxConsecutiveFailures).toBeGreaterThan(0);
    expect(DEFAULT_VENUE_HEALTH_CONFIG.cooldownMs).toBeGreaterThan(0);
    expect(DEFAULT_VENUE_HEALTH_CONFIG.latencyFailMs).toBeGreaterThan(0);
  });
});
