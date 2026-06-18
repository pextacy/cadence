import { PRICE_SCALE, BPS_DENOMINATOR } from "@cadence/mandate";

export { PRICE_SCALE, BPS_DENOMINATOR };

/**
 * The minimum acceptable output for a slice given a quote and a slippage cap:
 * `quotedOut * (BPS_DENOMINATOR - maxSlippageBps) / BPS_DENOMINATOR`, rounded
 * down. This is the `min_out` the executor commits to and the vault enforces.
 */
export function computeMinOut(quotedOut: bigint, maxSlippageBps: number): bigint {
  if (quotedOut <= 0n) throw new Error("quotedOut must be positive");
  if (maxSlippageBps < 0 || maxSlippageBps > BPS_DENOMINATOR) {
    throw new Error("maxSlippageBps out of range");
  }
  const keep = BigInt(BPS_DENOMINATOR - maxSlippageBps);
  return (quotedOut * keep) / BigInt(BPS_DENOMINATOR);
}

/**
 * The slippage implied by a (quotedOut, minOut) pair, in basis points, rounded
 * down — matching the contract's integer check
 * `(quotedOut - minOut) * BPS_DENOMINATOR / quotedOut`.
 */
export function impliedSlippageBps(quotedOut: bigint, minOut: bigint): number {
  if (quotedOut <= 0n) throw new Error("quotedOut must be positive");
  if (minOut < 0n) throw new Error("minOut must be non-negative");
  const bps = ((quotedOut - minOut) * BigInt(BPS_DENOMINATOR)) / quotedOut;
  return Number(bps);
}

/**
 * The execution price of a slice in fixed point (buy units per sell unit):
 * `quotedOut * PRICE_SCALE / sellAmount`, matching the contract.
 */
export function priceFixed(quotedOut: bigint, sellAmount: bigint): bigint {
  if (sellAmount <= 0n) throw new Error("sellAmount must be positive");
  return (quotedOut * PRICE_SCALE) / sellAmount;
}

/**
 * Whether `price` (fixed point) lies within `[floor, ceiling]`. A bound of `0n`
 * is treated as unset, matching the contract.
 */
export function withinBand(price: bigint, floor: bigint, ceiling: bigint): boolean {
  if (floor > 0n && price < floor) return false;
  if (ceiling > 0n && price > ceiling) return false;
  return true;
}
