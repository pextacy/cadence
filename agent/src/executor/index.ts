import type { CsprTradeClient } from "../clients/csprTrade.js";
import type { VaultClient } from "../clients/vault.js";
import type { Quote, RuntimeMandate, SliceProposal, VaultState } from "../types.js";
import { validateSlice, type GuardrailCode } from "./guardrails.js";

export type ExecOutcome =
  | {
      status: "filled";
      sliceId: number;
      sliceTxHash: string;
      swapDeployHash: string;
      attestTxHash: string;
      sellAmount: bigint;
      boughtAmount: bigint;
      minOut: bigint;
    }
  | { status: "skipped"; code: GuardrailCode; message: string }
  | { status: "paused"; reason: string };

export interface ExecutorDeps {
  vault: VaultClient;
  market: CsprTradeClient;
  sellToken: string;
  buyToken: string;
  /** Recipient of swap proceeds — the treasury's Casper account (non-custodial;
   * the agent never receives funds). */
  proceedsRecipient: string;
}

/** Sleep helper for backoff. */
const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/**
 * Retry a transient operation with exponential backoff. Deterministic in shape;
 * only the wall-clock delay varies.
 */
export async function withRetry<T>(
  fn: () => Promise<T>,
  attempts = 3,
  baseDelayMs = 500,
): Promise<T> {
  let lastErr: unknown;
  for (let i = 0; i < attempts; i++) {
    try {
      return await fn();
    } catch (err) {
      lastErr = err;
      if (i < attempts - 1) await delay(baseDelayMs * 2 ** i);
    }
  }
  throw lastErr;
}

/**
 * The deterministic executor. Given a planner proposal and current state it:
 * fetches a fresh quote, pre-checks every guardrail locally, submits the
 * on-chain `execute_slice` (which re-validates and enforces), performs the swap
 * at the venue, records the fill, and writes the decision attestation.
 *
 * It never makes LLM calls and never uses randomness for decisions. On any
 * ambiguity, quote failure or repeated revert it returns `paused` rather than
 * guessing — fail safe, not open.
 */
export class Executor {
  constructor(private readonly deps: ExecutorDeps) {}

  async executeOnce(
    mandate: RuntimeMandate,
    state: VaultState,
    proposal: SliceProposal,
    nowMs: number,
  ): Promise<ExecOutcome> {
    let quote: Quote;
    try {
      quote = await withRetry(() =>
        this.deps.market.getQuote({
          tokenIn: this.deps.sellToken,
          tokenOut: this.deps.buyToken,
          amount: proposal.sellAmount,
        }),
      );
    } catch (err) {
      return { status: "paused", reason: `quote fetch failed: ${describe(err)}` };
    }

    const check = validateSlice(mandate, state, proposal, quote, nowMs);
    if (!check.ok) {
      // Cap/deadline are normal terminal conditions; a price/slippage/venue
      // breach means the market or planner is out of bounds — skip this slice.
      return { status: "skipped", code: check.code, message: check.message };
    }
    const minOut = check.minOut;
    const sliceId = state.sliceCount;

    let sliceTxHash: string;
    try {
      sliceTxHash = await withRetry(() =>
        this.deps.vault.executeSlice({
          sellAmount: proposal.sellAmount,
          quotedOut: quote.quotedOut,
          minOut,
          venue: quote.venue,
        }),
      );
    } catch (err) {
      return { status: "paused", reason: `execute_slice reverted/failed: ${describe(err)}` };
    }

    let swap: { deployHash: string; boughtAmount: bigint };
    try {
      swap = await withRetry(() =>
        this.deps.market.executeSwap({
          tokenIn: this.deps.sellToken,
          tokenOut: this.deps.buyToken,
          amount: proposal.sellAmount,
          minOut,
          recipient: this.deps.proceedsRecipient,
          ...(quote.routeId ? { routeId: quote.routeId } : {}),
        }),
      );
    } catch (err) {
      return { status: "paused", reason: `swap submission failed: ${describe(err)}` };
    }

    if (swap.boughtAmount < minOut) {
      return {
        status: "paused",
        reason: `realised output ${swap.boughtAmount} below committed min_out ${minOut}`,
      };
    }

    let recordTxHash: string;
    try {
      recordTxHash = await withRetry(() =>
        this.deps.vault.recordFill({
          sliceId,
          boughtAmount: swap.boughtAmount,
          swapDeployHash: swap.deployHash,
        }),
      );
    } catch (err) {
      return { status: "paused", reason: `record_fill failed: ${describe(err)}` };
    }
    void recordTxHash;

    let attestTxHash: string;
    try {
      attestTxHash = await withRetry(() =>
        this.deps.vault.attest({ sliceId, reason: proposal.reason }),
      );
    } catch (err) {
      // A slice without an attestation is treated as failed (no silent trades).
      return { status: "paused", reason: `attestation failed: ${describe(err)}` };
    }

    return {
      status: "filled",
      sliceId,
      sliceTxHash,
      swapDeployHash: swap.deployHash,
      attestTxHash,
      sellAmount: proposal.sellAmount,
      boughtAmount: swap.boughtAmount,
      minOut,
    };
  }
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
