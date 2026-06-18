/**
 * Volatility- and failure-aware circuit breaker. A deterministic state machine
 * the loop consults before each slice: when the market is too volatile, or too
 * many slices in a row fail, it trips **open** and the agent pauses rather than
 * guessing — fail safe, not open. Pure: no I/O, no randomness. Lives in the
 * executor layer because, like the guardrails, it must be deterministic.
 */

export type BreakerState = "closed" | "open";

/** Outcome of the previous slice attempt, used to track the failure streak. */
export type SliceOutcomeKind = "filled" | "skipped" | "paused";

export interface BreakerConfig {
  /** Trip open when volatility (bps) reaches or exceeds this. */
  readonly volatilityTripBps: number;
  /** Trip open after this many consecutive non-fills (skips/pauses). */
  readonly maxConsecutiveFailures: number;
  /** Re-close only once volatility falls to or below this (hysteresis band). */
  readonly volatilityResetBps: number;
}

export const DEFAULT_BREAKER_CONFIG: BreakerConfig = {
  volatilityTripBps: 1_500,
  maxConsecutiveFailures: 3,
  volatilityResetBps: 750,
};

export interface BreakerSnapshot {
  readonly state: BreakerState;
  readonly consecutiveFailures: number;
  /** Why the breaker is currently open, recorded for logs/attestation. */
  readonly reason?: string;
}

export interface BreakerObservation {
  /** Current volatility in bps, if known (purchased or realised). */
  readonly volatilityBps?: number;
  /** The previous slice's outcome, if any. */
  readonly lastOutcome?: SliceOutcomeKind;
}

/** The breaker's starting state: closed, no failures. */
export const INITIAL_BREAKER: BreakerSnapshot = { state: "closed", consecutiveFailures: 0 };

/**
 * Advance the breaker by one observation. Returns a new snapshot (never mutates
 * the input). Transition rules:
 *  - A fill clears the failure streak; a skip/pause increments it.
 *  - When closed: trip open if volatility ≥ trip, or the streak hits the max.
 *  - When open: re-close only once volatility ≤ reset *and* the streak is below
 *    the max (hysteresis prevents flapping around the threshold).
 */
export function evaluateBreaker(
  prev: BreakerSnapshot,
  obs: BreakerObservation,
  cfg: BreakerConfig = DEFAULT_BREAKER_CONFIG,
): BreakerSnapshot {
  let failures = prev.consecutiveFailures;
  if (obs.lastOutcome === "filled") failures = 0;
  else if (obs.lastOutcome === "skipped" || obs.lastOutcome === "paused") failures += 1;

  const vol = obs.volatilityBps;

  if (prev.state === "open") {
    const calm = vol !== undefined && vol <= cfg.volatilityResetBps;
    if (calm && failures < cfg.maxConsecutiveFailures) {
      return { state: "closed", consecutiveFailures: failures };
    }
    return { state: "open", consecutiveFailures: failures, reason: prev.reason };
  }

  if (vol !== undefined && vol >= cfg.volatilityTripBps) {
    return {
      state: "open",
      consecutiveFailures: failures,
      reason: `volatility ${vol}bps ≥ trip ${cfg.volatilityTripBps}bps`,
    };
  }
  if (failures >= cfg.maxConsecutiveFailures) {
    return {
      state: "open",
      consecutiveFailures: failures,
      reason: `${failures} consecutive non-fills`,
    };
  }
  return { state: "closed", consecutiveFailures: failures };
}
