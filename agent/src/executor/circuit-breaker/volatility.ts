import { BPS_DENOMINATOR } from "../../units.js";

/**
 * Realised volatility in basis points from a window of fixed-point mid prices:
 * the population standard deviation of successive simple returns, scaled to bps.
 *
 * Pure and deterministic. Returns `undefined` for fewer than two prices (no
 * return series) so the caller can fall back to purchased volatility or treat the
 * signal as unknown. Uses only prices the agent already collects — no fabricated
 * data.
 */
export function realisedVolatilityBps(prices: readonly bigint[]): number | undefined {
  if (prices.length < 2) return undefined;

  const returns: number[] = [];
  for (let i = 1; i < prices.length; i++) {
    const prev = prices[i - 1];
    const curr = prices[i];
    if (prev === undefined || curr === undefined || prev <= 0n) continue;
    returns.push(Number(curr - prev) / Number(prev));
  }
  if (returns.length === 0) return undefined;

  const mean = returns.reduce((a, b) => a + b, 0) / returns.length;
  const variance = returns.reduce((a, b) => a + (b - mean) ** 2, 0) / returns.length;
  const bps = Math.round(Math.sqrt(variance) * BPS_DENOMINATOR);
  // A non-finite result (anomalous price feed) must not read as "calm" and
  // silently suppress a breaker trip — treat it as unknown instead.
  return Number.isFinite(bps) ? bps : undefined;
}
