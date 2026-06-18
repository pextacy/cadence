/**
 * Quote freshness guard. A {@link Quote} commits `quotedOut`/`minOut` on-chain in
 * `execute_slice`; if the quote is stale by the time it reaches the contract, the
 * agent commits to a price the market has already left — locking in slippage or
 * a guaranteed revert. This module rejects quotes older than a TTL so the
 * executor can refetch before committing.
 *
 * Pure and deterministic, mirroring the circuit-breaker style: no I/O, no
 * randomness, and `nowMs` is always injected so the check is reproducible. The
 * guard only needs a quote's timestamp, so it accepts the minimal shape below
 * rather than the full `Quote` (which carries `quotedAtMs` once stamped by the
 * CSPR.trade client).
 */

/** The only field this guard reads: the unix-ms the quote was produced. */
export interface TimestampedQuote {
  /** Unix ms the quote was produced by the venue. */
  readonly quotedAtMs: number;
}

export interface FreshnessConfig {
  /** Maximum age (ms) a quote may have when checked. Older quotes are rejected. */
  readonly ttlMs: number;
}

export const DEFAULT_FRESHNESS_CONFIG: FreshnessConfig = {
  ttlMs: 5_000,
};

export type FreshnessResult =
  | { readonly fresh: true; readonly ageMs: number }
  | { readonly fresh: false; readonly ageMs: number; readonly reason: string };

/**
 * Classify a quote as fresh or stale relative to `nowMs`. Age is clamped at 0:
 * a `quotedAtMs` in the future (clock skew) is treated as age 0 (fresh), never a
 * negative age. A non-finite or missing timestamp is treated as stale so a
 * malformed quote can never read as fresh and be committed on-chain.
 */
export function checkFreshness(
  quote: TimestampedQuote,
  nowMs: number,
  cfg: FreshnessConfig = DEFAULT_FRESHNESS_CONFIG,
): FreshnessResult {
  const stamped = quote.quotedAtMs;
  if (!Number.isFinite(stamped)) {
    return { fresh: false, ageMs: Number.POSITIVE_INFINITY, reason: "quote has no usable timestamp" };
  }
  const ageMs = Math.max(0, nowMs - stamped);
  if (ageMs > cfg.ttlMs) {
    return { fresh: false, ageMs, reason: `quote age ${ageMs}ms exceeds TTL ${cfg.ttlMs}ms` };
  }
  return { fresh: true, ageMs };
}

/** Boolean convenience wrapper around {@link checkFreshness}. */
export function isFresh(
  quote: TimestampedQuote,
  nowMs: number,
  cfg: FreshnessConfig = DEFAULT_FRESHNESS_CONFIG,
): boolean {
  return checkFreshness(quote, nowMs, cfg).fresh;
}
