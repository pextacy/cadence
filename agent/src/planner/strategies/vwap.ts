import { BPS_DENOMINATOR } from "../../units.js";
import type { StrategyFn } from "./types.js";
import { twap } from "./twap.js";

/**
 * Maximum share (in bps) of the *observed* sell-side depth a single slice will
 * target. 2_000 bps = 20% participation.
 */
export const VWAP_PARTICIPATION_BPS = 2_000;

/**
 * VWAP — volume-weighted slicing. Caps the time-weighted slice to a participation
 * rate of the depth actually observed in the market snapshot (real x402 depth
 * data). When depth is unknown it degrades to pure TWAP rather than inventing a
 * volume curve. Pure and deterministic.
 */
export const vwap: StrategyFn = (input) => {
  const base = twap(input);
  const depth = input.market.depthSell;
  if (depth === undefined || depth <= 0n) return base;
  const cap = (depth * BigInt(VWAP_PARTICIPATION_BPS)) / BigInt(BPS_DENOMINATOR);
  if (cap <= 0n) return base;
  const sliceSize = base.sliceSize < cap ? base.sliceSize : cap;
  return { sliceSize, suggestedSlippageBps: base.suggestedSlippageBps };
};
