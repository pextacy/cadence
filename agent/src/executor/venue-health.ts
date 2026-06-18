/**
 * Per-venue health circuit breaker. A deterministic state machine layered on top
 * of the volatility/failure breaker in `circuit-breaker/breaker.ts`: instead of
 * pausing the *whole* mandate when one venue misbehaves, it quarantines just that
 * venue — ejecting it from the routable allowlist for a cooldown after repeated
 * stale/failed quotes (or swaps) — while the other venues keep executing.
 *
 * Pure and immutable, mirroring `evaluateBreaker`: every transition returns a new
 * {@link VenueHealthTracker} and never mutates the input. No I/O, no randomness,
 * no wall-clock reads — `nowMs` is always injected so behaviour is reproducible.
 */

/** Outcome of a single quote or swap attempt against a venue. */
export type VenueOutcome = "ok" | "fail";

/** Health state of one venue. `up` = routable, `cooling` = quarantined. */
export type VenueState = "up" | "cooling";

export interface VenueHealthConfig {
  /** Quarantine a venue once it reaches this many consecutive failures. */
  readonly maxConsecutiveFailures: number;
  /** How long (ms) a venue stays quarantined before it may be probed again. */
  readonly cooldownMs: number;
  /** A latency at or above this (ms) counts a successful call as a soft failure. */
  readonly latencyFailMs: number;
}

export const DEFAULT_VENUE_HEALTH_CONFIG: VenueHealthConfig = {
  maxConsecutiveFailures: 3,
  cooldownMs: 60_000,
  latencyFailMs: 10_000,
};

/** Immutable per-venue record. Exposed via {@link VenueHealthTracker.snapshot}. */
export interface VenueRecord {
  readonly state: VenueState;
  /** Consecutive failures since the last clean success. */
  readonly failures: number;
  /** Wall-clock ms until which the venue stays quarantined (0 when `up`). */
  readonly cooldownUntilMs: number;
  /** Latency (ms) of the most recent recorded call, for observability. */
  readonly lastLatencyMs: number;
}

/** What `snapshot()` returns: the externally-visible slice of each record. */
export type VenueHealthSnapshot = Readonly<
  Record<string, { state: VenueState; failures: number; cooldownUntilMs: number }>
>;

const FRESH_RECORD: VenueRecord = {
  state: "up",
  failures: 0,
  cooldownUntilMs: 0,
  lastLatencyMs: 0,
};

/**
 * Deterministic per-venue breaker. Construct empty (every venue assumed healthy)
 * and fold outcomes into it with {@link record}; query the routable subset with
 * {@link healthy}.
 */
export class VenueHealthTracker {
  private constructor(
    private readonly records: ReadonlyMap<string, VenueRecord>,
    private readonly cfg: VenueHealthConfig,
  ) {}

  /** A tracker with no recorded history (all venues healthy). */
  static empty(cfg: VenueHealthConfig = DEFAULT_VENUE_HEALTH_CONFIG): VenueHealthTracker {
    return new VenueHealthTracker(new Map(), cfg);
  }

  /**
   * Fold one quote/swap outcome for `venue` into the tracker, returning a NEW
   * tracker (never mutates). Transition rules:
   *  - `ok` under the latency limit clears the failure streak and keeps/returns
   *    the venue to `up` (cooldown lifted).
   *  - `ok` at or above `latencyFailMs` is treated as a soft failure (slow venue).
   *  - `fail` (or a slow `ok`) increments the streak; reaching
   *    `maxConsecutiveFailures` quarantines the venue (`cooling`) until
   *    `nowMs + cooldownMs`.
   */
  record(venue: string, outcome: VenueOutcome, latencyMs: number, nowMs: number): VenueHealthTracker {
    const prev = this.records.get(venue) ?? FRESH_RECORD;
    const slow = latencyMs >= this.cfg.latencyFailMs;
    const failed = outcome === "fail" || slow;

    let next: VenueRecord;
    if (!failed) {
      // Clean, fast success: fully heal.
      next = { state: "up", failures: 0, cooldownUntilMs: 0, lastLatencyMs: latencyMs };
    } else {
      const failures = prev.failures + 1;
      if (failures >= this.cfg.maxConsecutiveFailures) {
        next = {
          state: "cooling",
          failures,
          cooldownUntilMs: nowMs + this.cfg.cooldownMs,
          lastLatencyMs: latencyMs,
        };
      } else {
        next = {
          state: prev.state,
          failures,
          cooldownUntilMs: prev.cooldownUntilMs,
          lastLatencyMs: latencyMs,
        };
      }
    }

    const records = new Map(this.records);
    records.set(venue, next);
    return new VenueHealthTracker(records, this.cfg);
  }

  /** True if the venue is routable right now (never quarantined, or cooled off). */
  isHealthy(venue: string, nowMs: number): boolean {
    const rec = this.records.get(venue);
    if (rec === undefined || rec.state === "up") return true;
    return nowMs >= rec.cooldownUntilMs;
  }

  /**
   * Filter `allowlist` down to the venues healthy enough to route to at `nowMs`.
   * Order is preserved. A venue whose cooldown has elapsed is eligible again
   * (probationary): the next failure re-quarantines it.
   */
  healthy(allowlist: readonly string[], nowMs: number): readonly string[] {
    return allowlist.filter((v) => this.isHealthy(v, nowMs));
  }

  /** Externally-visible health of every venue with recorded history. */
  snapshot(): VenueHealthSnapshot {
    const out: Record<string, { state: VenueState; failures: number; cooldownUntilMs: number }> = {};
    for (const [venue, rec] of this.records) {
      out[venue] = {
        state: rec.state,
        failures: rec.failures,
        cooldownUntilMs: rec.cooldownUntilMs,
      };
    }
    return out;
  }
}
