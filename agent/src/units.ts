import { PRICE_SCALE, BPS_DENOMINATOR } from "@cadence/mandate";

export { PRICE_SCALE, BPS_DENOMINATOR };

/**
 * The minimum acceptable output for a slice given a quote and a slippage cap:
 * `quotedOut * (BPS_DENOMINATOR - maxSlippageBps) / BPS_DENOMINATOR`, rounded
 * **up**. This is the `min_out` the executor commits to and the vault enforces.
 *
 * Rounding UP (not down) is required for parity with the vault: the contract's
 * check is the cross-multiply `(quotedOut - min_out) * BPS <= quotedOut * bps`.
 * A floored min_out can have an implied slippage that exceeds the cap by one
 * integer unit — the agent's back-computed (also floored) check would pass it but
 * `execute_slice` would revert. Rounding min_out up guarantees it satisfies the
 * contract predicate (see {@link withinSlippage}).
 */
export function computeMinOut(quotedOut: bigint, maxSlippageBps: number): bigint {
  if (quotedOut <= 0n) throw new Error("quotedOut must be positive");
  if (maxSlippageBps < 0 || maxSlippageBps > BPS_DENOMINATOR) {
    throw new Error("maxSlippageBps out of range");
  }
  const keep = BigInt(BPS_DENOMINATOR - maxSlippageBps);
  const denom = BigInt(BPS_DENOMINATOR);
  return (quotedOut * keep + denom - 1n) / denom;
}

/**
 * The vault's exact slippage predicate, mirrored byte-for-byte:
 * `(quotedOut - minOut) * BPS_DENOMINATOR <= quotedOut * maxSlippageBps`.
 *
 * Using the cross-multiply form (rather than a back-computed, floored bps) makes
 * the agent's pre-check agree with `execute_slice` on every input — no rounding
 * window where the agent submits a slice the contract reverts.
 */
export function withinSlippage(quotedOut: bigint, minOut: bigint, maxSlippageBps: number): boolean {
  if (quotedOut <= 0n) throw new Error("quotedOut must be positive");
  if (minOut < 0n || minOut > quotedOut) return false;
  const lhs = (quotedOut - minOut) * BigInt(BPS_DENOMINATOR);
  const rhs = quotedOut * BigInt(maxSlippageBps);
  return lhs <= rhs;
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
