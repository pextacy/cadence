import type { StrategyFn } from "./types.js";

/**
 * Time-weighted even split: divide the remaining size by the slices remaining,
 * rounded down. Returns the whole remainder when no slices are left to schedule.
 * Pure and deterministic — the shared primitive every strategy builds on.
 */
export function evenSplit(remaining: bigint, slicesRemaining: number): bigint {
  if (slicesRemaining <= 0) return remaining;
  return remaining / BigInt(slicesRemaining);
}

/** TWAP — spread the order evenly across the remaining schedule. */
export const twap: StrategyFn = ({ remaining, slicesRemaining, mandate }) => ({
  sliceSize: evenSplit(remaining, slicesRemaining),
  suggestedSlippageBps: mandate.maxSlippageBps,
});
