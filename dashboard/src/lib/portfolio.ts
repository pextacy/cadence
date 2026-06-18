import { PRICE_SCALE } from "@cadence/mandate";
import type { DashboardState } from "../types.js";

/** Aggregate, desk-wide figures across every mandate in the portfolio. */
export interface PortfolioSummary {
  mandateCount: number;
  activeCount: number;
  pausedCount: number;
  /** Completed + Expired (i.e. settled, terminal). */
  completedCount: number;
  totalRemaining: bigint;
  totalSold: bigint;
  totalBought: bigint;
  /** Aggregate realised price (fixed point): totalBought ÷ totalSold. Null until a fill. */
  averagePrice: bigint | null;
  /** Earliest deadline among non-settled tracks (unix ms), or null. */
  nearestDeadlineMs: number | null;
}

/**
 * Fold a set of per-vault dashboard states into one portfolio summary. Pure: no
 * I/O, derived entirely from the reconstructed on-chain state of each vault.
 */
export function aggregatePortfolio(
  states: readonly DashboardState[],
  _nowMs: number,
): PortfolioSummary {
  let activeCount = 0;
  let pausedCount = 0;
  let completedCount = 0;
  let totalRemaining = 0n;
  let totalSold = 0n;
  let totalBought = 0n;
  let nearestDeadlineMs: number | null = null;

  for (const s of states) {
    if (s.status === "Active") activeCount += 1;
    else if (s.status === "Paused") pausedCount += 1;
    else if (s.status === "Completed" || s.status === "Expired") completedCount += 1;

    totalRemaining += s.totalSell > s.soldSoFar ? s.totalSell - s.soldSoFar : 0n;
    totalSold += s.soldSoFar;
    totalBought += s.boughtSoFar;

    const settled = s.status === "Completed" || s.status === "Expired";
    if (!settled && s.endTimeMs !== undefined) {
      nearestDeadlineMs =
        nearestDeadlineMs === null ? s.endTimeMs : Math.min(nearestDeadlineMs, s.endTimeMs);
    }
  }

  const averagePrice = totalSold > 0n ? (totalBought * PRICE_SCALE) / totalSold : null;

  return {
    mandateCount: states.length,
    activeCount,
    pausedCount,
    completedCount,
    totalRemaining,
    totalSold,
    totalBought,
    averagePrice,
    nearestDeadlineMs,
  };
}
