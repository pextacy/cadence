import type { Config } from "./config.js";
import type { CsprTradeClient } from "./clients/csprTrade.js";
import type { VaultClient } from "./clients/vault.js";
import { fetchWithX402 } from "./clients/x402.js";
import { priceFixed } from "./units.js";
import type { MarketSnapshot } from "./types.js";

/** Reference quote size used to derive a mid price each tick, in base units. */
export const REFERENCE_QUOTE_AMOUNT = 1_000_000n;
/** Target number of slices across a mandate window (TWAP granularity). */
export const TARGET_SLICES = 10;
/** Mid-price samples kept to estimate realised volatility when none is purchased. */
export const PRICE_HISTORY_MAX = 20;

/** A structured log line the dashboard/operator can follow. */
export function log(event: string, detail: Record<string, unknown> = {}): void {
  console.log(JSON.stringify({ ts: new Date().toISOString(), event, ...detail }));
}

/** Promise-based sleep. */
export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/**
 * Opportunistically push the vault's accumulated protocol fee to the wired fee
 * module via the decoupled `flush_fees` entrypoint, after a run finalises.
 *
 * Fee flushing is non-essential bookkeeping (CLAUDE.md §4.6 fail-safe): a fee
 * that did not flush is acceptable, a broken/blocked run is not. So EVERY error
 * is swallowed and logged as an informational no-op — never rethrown. In
 * particular `FeeNotActive` (no fee module wired) and `NothingToFlush` (nothing
 * accumulated) are the expected benign reverts and must not surface as failures.
 *
 * Side-effecting on-chain call kept deterministic in shape (single attempt, no
 * retry/randomness) and audited via the same structured `log` the loop uses.
 */
export async function flushFeesBestEffort(
  vault: Pick<VaultClient, "flushFees">,
  detail: Record<string, unknown> = {},
): Promise<void> {
  try {
    const flushTx = await vault.flushFees();
    log("fees_flushed", { ...detail, flushTx });
  } catch (err) {
    // Benign "nothing to do" (FeeNotActive/NothingToFlush) and any other flush
    // failure are non-fatal: log and continue so settlement is never blocked.
    log("fees_flush_skipped", {
      ...detail,
      reason: err instanceof Error ? err.message : String(err),
    });
  }
}

/**
 * The bounded delay (ms) to wait before submitting a slice the planner scheduled
 * for `notBeforeMs`. This is how the loop honours the strategy's pacing (TWAP/VWAP
 * spread a child order across the window); without it every slice fires back-to-back
 * at the poll cadence and the time-weighting is silently lost.
 *
 * Returns 0 when the slice is already due (or its scheduled time is in the past or
 * non-finite); never negative; and is capped to the mandate deadline so a slice
 * scheduled beyond the window can never block the loop past `deadlineMs`.
 */
export function submissionDelayMs(
  notBeforeMs: number,
  nowMs: number,
  deadlineMs: number,
): number {
  if (!Number.isFinite(notBeforeMs)) return 0;
  const untilDue = notBeforeMs - nowMs;
  if (untilDue <= 0) return 0;
  const untilDeadline = Math.max(0, deadlineMs - nowMs);
  return Math.min(untilDue, untilDeadline);
}

/**
 * Build a market snapshot for the given `sellAsset`/`buyAsset` pair, paying for
 * premium depth/volatility via x402 when a resource is configured. Returns a
 * fully-constructed snapshot (no mutation); degrades to mid-price-only when the
 * premium call is absent or fails.
 */
export async function buildSnapshot(
  cfg: Config,
  market: CsprTradeClient,
  agentAccountHash: string,
  sellAsset: string,
  buyAsset: string,
): Promise<MarketSnapshot> {
  const ref = await market.getQuote({
    tokenIn: sellAsset,
    tokenOut: buyAsset,
    amount: REFERENCE_QUOTE_AMOUNT,
  });
  const midPrice = priceFixed(ref.quotedOut, REFERENCE_QUOTE_AMOUNT);
  const base: MarketSnapshot = { midPrice, takenAtMs: Date.now() };

  if (!cfg.x402DepthResource) return base;
  try {
    const { data, proof } = await fetchWithX402<{
      volatility_bps?: number;
      depth_sell?: string;
    }>({
      resourceUrl: cfg.x402DepthResource,
      network: `casper:${cfg.chainName}`,
      from: agentAccountHash,
      privateKeyHex: cfg.agentPrivateKeyHex,
    });
    log("x402_payment", { resource: proof.resource, amount: proof.amount, asset: proof.asset });
    return {
      ...base,
      ...(typeof data.volatility_bps === "number" ? { volatilityBps: data.volatility_bps } : {}),
      ...(data.depth_sell ? { depthSell: BigInt(data.depth_sell) } : {}),
    };
  } catch (err) {
    log("x402_skipped", { reason: err instanceof Error ? err.message : String(err) });
    return base;
  }
}
