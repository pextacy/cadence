import type { Config } from "./config.js";
import type { CsprTradeClient } from "./clients/csprTrade.js";
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
