import { describe, it, expect } from "vitest";
import {
  checkFreshness,
  isFresh,
  DEFAULT_FRESHNESS_CONFIG,
  type FreshnessConfig,
} from "./quote-freshness.js";

const cfg: FreshnessConfig = { ttlMs: 5_000 };
const T0 = 1_000_000;

describe("checkFreshness", () => {
  it("accepts a quote within the TTL", () => {
    const r = checkFreshness({ quotedAtMs: T0 }, T0 + 4_000, cfg);
    expect(r.fresh).toBe(true);
    expect(r.ageMs).toBe(4_000);
  });

  it("accepts a quote exactly at the TTL boundary", () => {
    expect(checkFreshness({ quotedAtMs: T0 }, T0 + 5_000, cfg).fresh).toBe(true);
  });

  it("rejects a quote older than the TTL", () => {
    const r = checkFreshness({ quotedAtMs: T0 }, T0 + 5_001, cfg);
    expect(r.fresh).toBe(false);
    if (!r.fresh) expect(r.reason).toMatch(/exceeds TTL/);
  });

  it("clamps future timestamps (clock skew) to age 0 rather than negative", () => {
    const r = checkFreshness({ quotedAtMs: T0 + 1_000 }, T0, cfg);
    expect(r.fresh).toBe(true);
    expect(r.ageMs).toBe(0);
  });

  it("treats a non-finite timestamp as stale", () => {
    const r = checkFreshness({ quotedAtMs: Number.NaN }, T0, cfg);
    expect(r.fresh).toBe(false);
  });

  it("isFresh mirrors checkFreshness", () => {
    expect(isFresh({ quotedAtMs: T0 }, T0 + 1_000, cfg)).toBe(true);
    expect(isFresh({ quotedAtMs: T0 }, T0 + 9_000, cfg)).toBe(false);
  });

  it("ships a sane default TTL", () => {
    expect(DEFAULT_FRESHNESS_CONFIG.ttlMs).toBeGreaterThan(0);
  });
});
