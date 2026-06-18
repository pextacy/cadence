import type { MarketSnapshot, RuntimeMandate } from "../../types.js";

/** Inputs a strategy needs to size the next child order. All deterministic. */
export interface StrategyInput {
  /** Remaining size still to sell, in sell-asset base units. */
  readonly remaining: bigint;
  /** Slices left in the target schedule (≥ 1 by the time it reaches a strategy). */
  readonly slicesRemaining: number;
  /** The signed mandate limits. */
  readonly mandate: RuntimeMandate;
  /** The current market snapshot (mid price, optional purchased depth/volatility). */
  readonly market: MarketSnapshot;
}

/**
 * A strategy's suggested next-slice shape. This is *guidance* fed to the planner
 * LLM and is never authoritative — the deterministic executor and the on-chain
 * vault re-validate every field before any funds move.
 */
export interface StrategyOutput {
  /** Suggested next-slice size in sell-asset base units. */
  readonly sliceSize: bigint;
  /** Suggested per-slice slippage cap in bps (always ≤ the mandate cap). */
  readonly suggestedSlippageBps: number;
}

/** A pure, deterministic slice-sizing strategy. No I/O, no randomness. */
export type StrategyFn = (input: StrategyInput) => StrategyOutput;
