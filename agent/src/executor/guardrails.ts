import type { Quote, RuntimeMandate, SliceProposal, VaultState } from "../types.js";
import { impliedSlippageBps, priceFixed, withinBand } from "../units.js";

/**
 * Guardrail rejection codes, named to mirror the on-chain `Error` variants. The
 * local pre-check exists only to avoid wasting a transaction; the contract is the
 * authority and re-validates every one of these on-chain.
 */
export type GuardrailCode =
  | "NotActive"
  | "DeadlinePassed"
  | "ZeroAmount"
  | "MinOutAboveQuote"
  | "SpendCapExceeded"
  | "SlippageTooHigh"
  | "PriceOutOfBand"
  | "VenueNotAllowed";

export type GuardrailResult =
  | { ok: true; minOut: bigint }
  | { ok: false; code: GuardrailCode; message: string };

/**
 * Deterministically validate a planner proposal + venue quote against the mandate
 * and current vault state, exactly as the vault's `execute_slice` will. Returns
 * the committed `min_out` on success. No randomness, no I/O.
 */
export function validateSlice(
  mandate: RuntimeMandate,
  state: VaultState,
  proposal: SliceProposal,
  quote: Quote,
  nowMs: number,
): GuardrailResult {
  if (state.status !== "Active") {
    return { ok: false, code: "NotActive", message: `vault status is ${state.status}` };
  }
  if (nowMs > mandate.endTimeMs) {
    return { ok: false, code: "DeadlinePassed", message: "execution window has closed" };
  }
  if (proposal.sellAmount <= 0n || quote.quotedOut <= 0n) {
    return { ok: false, code: "ZeroAmount", message: "sell amount and quote must be positive" };
  }
  if (quote.sellAmount !== proposal.sellAmount) {
    return {
      ok: false,
      code: "ZeroAmount",
      message: "quote sell amount does not match the proposed slice",
    };
  }

  // The per-slice cap is the tighter of the mandate cap and the planner's request.
  const effectiveSlippageBps = Math.min(proposal.maxSlippageBps, mandate.maxSlippageBps);
  const minOut =
    (quote.quotedOut * BigInt(10_000 - effectiveSlippageBps)) / 10_000n;

  if (minOut > quote.quotedOut) {
    return { ok: false, code: "MinOutAboveQuote", message: "min_out exceeds quote" };
  }
  if (state.soldSoFar + proposal.sellAmount > mandate.totalSell) {
    return { ok: false, code: "SpendCapExceeded", message: "slice would exceed the spend cap" };
  }
  if (impliedSlippageBps(quote.quotedOut, minOut) > mandate.maxSlippageBps) {
    return { ok: false, code: "SlippageTooHigh", message: "implied slippage exceeds the cap" };
  }
  const price = priceFixed(quote.quotedOut, proposal.sellAmount);
  if (!withinBand(price, mandate.priceFloor, mandate.priceCeiling)) {
    return { ok: false, code: "PriceOutOfBand", message: "quoted price is outside the band" };
  }
  if (!mandate.venueAllowlist.includes(quote.venue)) {
    return { ok: false, code: "VenueNotAllowed", message: `venue ${quote.venue} not allowlisted` };
  }
  return { ok: true, minOut };
}
