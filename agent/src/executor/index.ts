import type { CsprTradeClient } from "../clients/csprTrade.js";
import type { VaultClient } from "../clients/vault.js";
import type { ConfirmationService } from "../clients/confirm.js";
import type { Quote, RuntimeMandate, SliceProposal, VaultState } from "../types.js";
import { selectBestQuote } from "../routing/best-execution.js";
import { validateSlice, type GuardrailCode } from "./guardrails.js";
import { checkFreshness, type FreshnessConfig } from "./quote-freshness.js";
import { VenueHealthTracker, type VenueHealthConfig } from "./venue-health.js";

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
  | { status: "skipped"; code: GuardrailCode | "StaleQuote" | "NoHealthyVenue"; message: string }
  | { status: "paused"; reason: string };

export interface ExecutorDeps {
  vault: VaultClient;
  market: CsprTradeClient;
  /** Polls submitted transactions/deploys to finality so the executor never
   * advances on an unconfirmed or reverted submission. */
  confirm: ConfirmationService;
  sellToken: string;
  buyToken: string;
  /** Recipient of swap proceeds — the treasury's Casper account (non-custodial;
   * the agent never receives funds). */
  proceedsRecipient: string;
  /** Quote freshness TTL; a quote older than this at commit time is rejected and
   * the slice is skipped so the loop refetches. Defaults to the module default. */
  freshness?: FreshnessConfig;
  /** Per-venue health breaker config; a venue is quarantined out of routing after
   * repeated quote/swap failures. Defaults to the module default. */
  venueHealth?: VenueHealthConfig;
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
  /** Per-venue health breaker, folded across this run's quote/swap outcomes so a
   * venue that repeatedly fails is quarantined out of routing for a cooldown. */
  private health: VenueHealthTracker;

  constructor(private readonly deps: ExecutorDeps) {
    this.health = VenueHealthTracker.empty(deps.venueHealth);
  }

  /** The externally-visible health of every venue with recorded history. */
  venueHealthSnapshot() {
    return this.health.snapshot();
  }

  async executeOnce(
    mandate: RuntimeMandate,
    state: VaultState,
    proposal: SliceProposal,
    nowMs: number,
  ): Promise<ExecOutcome> {
    // Route only over venues healthy enough right now: a venue quarantined by
    // repeated quote/swap failures is held out until its cooldown elapses, so one
    // misbehaving venue never stalls the whole mandate.
    const routable = this.health.healthy(mandate.venueAllowlist, nowMs);
    if (routable.length === 0) {
      return {
        status: "skipped",
        code: "NoHealthyVenue",
        message: "every allowlisted venue is quarantined (cooling); will retry after cooldown",
      };
    }

    // Best execution: quote every routable venue and take the best output. The
    // chosen venue is re-validated against the allowlist by both the guardrail
    // pre-check below and the vault on-chain.
    let quote: Quote;
    try {
      const quotes = await withRetry(() =>
        this.deps.market.getQuotes(
          {
            tokenIn: this.deps.sellToken,
            tokenOut: this.deps.buyToken,
            amount: proposal.sellAmount,
          },
          routable,
        ),
      );
      const best = selectBestQuote(quotes, routable);
      if (best === null) {
        // No usable quote from any routable venue: count a failure against each so
        // a persistently empty book eventually quarantines them.
        this.recordVenues(routable, "fail", nowMs);
        return { status: "skipped", code: "VenueNotAllowed", message: "no routable venue produced a usable quote" };
      }
      quote = best;
    } catch (err) {
      this.recordVenues(routable, "fail", nowMs);
      return { status: "paused", reason: `quote fetch failed: ${describe(err)}` };
    }

    // Reject a quote that has gone stale between fetch and now: committing a price
    // the market has already left locks in slippage or guarantees an on-chain
    // revert. Skipping (not pausing) lets the loop refetch on the next tick.
    const freshness = checkFreshness(quote, nowMs, this.deps.freshness);
    if (!freshness.fresh) {
      return { status: "skipped", code: "StaleQuote", message: freshness.reason };
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

    // Gate on finality: the slice releases the sell asset on-chain, so the swap
    // must never fire until that transaction is confirmed included AND did not
    // revert. A failed guardrail or a timeout leaves the vault for the treasury
    // rather than swapping against an unconfirmed slice — fail safe, not open.
    {
      const confirmed = await this.deps.confirm.confirmTransaction(sliceTxHash);
      if (confirmed.status !== "success") {
        return {
          status: "paused",
          reason:
            confirmed.status === "failure"
              ? `execute_slice reverted on-chain: ${confirmed.errorMessage}`
              : `execute_slice not confirmed within budget (${sliceTxHash})`,
        };
      }
    }

    // Realised output is measured as the treasury's buy-asset balance delta across
    // the confirmed swap — `build_swap` only knows the expected amount, never the
    // settled one. Snapshot the balance before submitting.
    let balanceBefore: bigint;
    try {
      balanceBefore = await withRetry(() =>
        this.deps.market.tokenBalance(this.deps.proceedsRecipient, this.deps.buyToken),
      );
    } catch (err) {
      return { status: "paused", reason: `buy-asset balance read failed: ${describe(err)}` };
    }

    let swap: { deployHash: string };
    try {
      swap = await withRetry(() =>
        this.deps.market.swap({
          tokenIn: this.deps.sellToken,
          tokenOut: this.deps.buyToken,
          amount: proposal.sellAmount,
          slippageBps: proposal.maxSlippageBps,
        }),
      );
    } catch (err) {
      // Swap failure is venue-attributable: count it against the chosen venue.
      this.recordVenues([quote.venue], "fail", nowMs);
      return { status: "paused", reason: `swap submission failed: ${describe(err)}` };
    }

    // Gate on the swap deploy landing on-chain before recording the fill: a
    // record_fill against an unconfirmed (or reverted) swap would attest proceeds
    // that never settled. Confirm the off-chain venue's deploy hash first.
    {
      const settled = await this.deps.confirm.confirmDeploy(swap.deployHash);
      if (settled.status !== "success") {
        // The venue's swap did not land: quarantine pressure on the chosen venue.
        this.recordVenues([quote.venue], "fail", nowMs);
        return {
          status: "paused",
          reason:
            settled.status === "failure"
              ? `swap deploy reverted on-chain: ${settled.errorMessage}`
              : `swap deploy not confirmed within budget (${swap.deployHash})`,
        };
      }
    }

    // The swap is settled: the balance delta is the realised, on-chain output.
    let boughtAmount: bigint;
    try {
      const after = await withRetry(() =>
        this.deps.market.tokenBalance(this.deps.proceedsRecipient, this.deps.buyToken),
      );
      boughtAmount = after - balanceBefore;
    } catch (err) {
      return { status: "paused", reason: `realised output read failed: ${describe(err)}` };
    }
    if (boughtAmount < minOut) {
      this.recordVenues([quote.venue], "fail", nowMs);
      return {
        status: "paused",
        reason: `realised output ${boughtAmount} below committed min_out ${minOut}`,
      };
    }

    let recordTxHash: string;
    try {
      recordTxHash = await withRetry(() =>
        this.deps.vault.recordFill({
          sliceId,
          boughtAmount,
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

    // Full success: the chosen venue quoted and settled cleanly — clear its
    // failure streak so a single past blip does not keep it under suspicion.
    this.recordVenues([quote.venue], "ok", nowMs);

    return {
      status: "filled",
      sliceId,
      sliceTxHash,
      swapDeployHash: swap.deployHash,
      attestTxHash,
      sellAmount: proposal.sellAmount,
      boughtAmount,
      minOut,
    };
  }

  /**
   * Fold a quote/swap outcome for one or more venues into the health tracker.
   * Latency is not separately measured here (the failure/success signal is what
   * drives quarantine), so a healthy latency of 0 is recorded for `ok`.
   */
  private recordVenues(venues: readonly string[], outcome: "ok" | "fail", nowMs: number): void {
    for (const venue of venues) {
      this.health = this.health.record(venue, outcome, 0, nowMs);
    }
  }
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
