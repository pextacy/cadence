import type { Strategy } from "../../types.js";
import type { StrategyFn } from "./types.js";
import { twap } from "./twap.js";
import { vwap } from "./vwap.js";
import { adaptive } from "./adaptive.js";

/** Every mandate {@link Strategy} mapped to its deterministic implementation. */
export const STRATEGIES: Record<Strategy, StrategyFn> = {
  TWAP: twap,
  VWAP: vwap,
  ADAPTIVE: adaptive,
};

/** Resolve the strategy implementation for a mandate, defaulting to TWAP. */
export function strategyFor(strategy: Strategy): StrategyFn {
  return STRATEGIES[strategy] ?? twap;
}

export type { StrategyInput, StrategyOutput, StrategyFn } from "./types.js";
