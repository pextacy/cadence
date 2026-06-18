import type { StrategyFn } from "./types.js";
import { twap } from "./twap.js";

/** Volatility (bps) at or below which the full TWAP slice size is used. */
export const ADAPTIVE_VOL_REFERENCE_BPS = 500;

/**
 * Floor on the slice size as a fraction (1 / divisor) of the TWAP size, so the
 * order keeps making progress even under sustained high volatility.
 */
export const ADAPTIVE_MIN_SIZE_DIVISOR = 10n;

/**
 * ADAPTIVE — volatility-aware slicing. As volatility rises above the reference,
 * shrink the slice and tighten the per-slice slippage cap inversely to the ratio,
 * floored so the order still progresses and the cap stays ≥ 1 bps. Falls back to
 * TWAP when volatility is unknown. Pure and deterministic.
 */
export const adaptive: StrategyFn = (input) => {
  const base = twap(input);
  const vol = input.market.volatilityBps;
  if (vol === undefined || vol <= ADAPTIVE_VOL_REFERENCE_BPS) return base;

  const ref = ADAPTIVE_VOL_REFERENCE_BPS;
  const scaled = (base.sliceSize * BigInt(ref)) / BigInt(vol);
  const floor = base.sliceSize / ADAPTIVE_MIN_SIZE_DIVISOR;
  const sliceSize = scaled > floor ? scaled : floor;
  const tightened = Math.max(1, Math.floor((input.mandate.maxSlippageBps * ref) / vol));

  return {
    sliceSize: sliceSize > 0n ? sliceSize : base.sliceSize,
    suggestedSlippageBps: tightened,
  };
};
