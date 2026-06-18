import { loadConfig } from "./config.js";
import { CsprTradeClient } from "./clients/csprTrade.js";
import { VaultClient } from "./clients/vault.js";
import { Planner } from "./planner/index.js";
import { Executor } from "./executor/index.js";
import {
  evaluateBreaker,
  INITIAL_BREAKER,
  DEFAULT_BREAKER_CONFIG,
  type BreakerSnapshot,
  type SliceOutcomeKind,
} from "./executor/circuit-breaker/breaker.js";
import { realisedVolatilityBps } from "./executor/circuit-breaker/volatility.js";
import { loadSignedMandate, toRuntimeMandate } from "./mandate.js";
import { buildSnapshot, log, sleep, TARGET_SLICES, PRICE_HISTORY_MAX } from "./runtime.js";
import type { MarketSnapshot, RuntimeMandate, VaultState } from "./types.js";

/** Run the full execution loop until the order completes or the window closes. */
export async function runAgent(): Promise<void> {
  const cfg = loadConfig();
  const signed = await loadSignedMandate(process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json");
  const mandate: RuntimeMandate = toRuntimeMandate(signed.mandate);

  const vault = new VaultClient({
    nodeRpcUrl: cfg.casperNodeRpc,
    chainName: cfg.chainName,
    contractHash: cfg.vaultContractHash,
    agentPrivateKeyHex: cfg.agentPrivateKeyHex,
  });
  const market = new CsprTradeClient(cfg.csprTradeMcpUrl, mandate.venueAllowlist[0] ?? "cspr.trade");
  await market.connect();

  const agentAccountHash = vault.agentAccountHash();
  const planner = new Planner(cfg.llmApiKey, cfg.llmModel);
  const executor = new Executor({
    vault,
    market,
    sellToken: mandate.sellAsset,
    buyToken: mandate.buyAsset,
    // Proceeds go to the treasury, never the agent — the agent custodies no funds.
    proceedsRecipient: cfg.treasuryAccountHash,
  });

  // The contract is the authority on state; the agent tracks its own submissions
  // and the dashboard reconstructs authoritative state from on-chain events.
  let state: VaultState = {
    status: "Active",
    soldSoFar: 0n,
    boughtSoFar: 0n,
    sliceCount: 0,
    totalSell: mandate.totalSell,
  };

  log("agent_start", {
    totalSell: mandate.totalSell.toString(),
    endTimeMs: mandate.endTimeMs,
    venue: mandate.venueAllowlist,
    strategy: mandate.strategy,
  });

  // The circuit breaker is consulted before each slice; price history backs the
  // realised-volatility estimate used when premium volatility is not purchased.
  let breaker: BreakerSnapshot = INITIAL_BREAKER;
  let lastOutcome: SliceOutcomeKind | undefined;
  let priceHistory: readonly bigint[] = [];
  // When the agent pauses (circuit breaker or executor), it leaves funds safe in
  // the vault rather than settling — settlement is only for natural completion.
  let paused = false;

  try {
    while (state.soldSoFar < mandate.totalSell && Date.now() <= mandate.endTimeMs) {
      const baseSnapshot = await buildSnapshot(cfg, market, agentAccountHash, mandate.sellAsset, mandate.buyAsset);
      priceHistory = [...priceHistory, baseSnapshot.midPrice].slice(-PRICE_HISTORY_MAX);

      // Prefer purchased volatility; otherwise fall back to a realised estimate
      // computed from the agent's own mid-price samples (no fabricated data).
      const volatilityBps = baseSnapshot.volatilityBps ?? realisedVolatilityBps(priceHistory);
      const snapshot: MarketSnapshot =
        volatilityBps === undefined ? baseSnapshot : { ...baseSnapshot, volatilityBps };

      breaker = evaluateBreaker(breaker, { volatilityBps, lastOutcome }, DEFAULT_BREAKER_CONFIG);
      if (breaker.state === "open") {
        log("circuit_breaker_open", { reason: breaker.reason, volatilityBps });
        await vault.pause();
        paused = true;
        break;
      }

      const proposal = await planner.propose({
        mandate,
        state,
        market: snapshot,
        nowMs: Date.now(),
        targetSlices: TARGET_SLICES,
      });
      log("proposal", {
        sellAmount: proposal.sellAmount.toString(),
        maxSlippageBps: proposal.maxSlippageBps,
        reason: proposal.reason,
      });

      const outcome = await executor.executeOnce(mandate, state, proposal, Date.now());
      if (outcome.status === "filled") {
        state = {
          ...state,
          soldSoFar: state.soldSoFar + outcome.sellAmount,
          boughtSoFar: state.boughtSoFar + outcome.boughtAmount,
          sliceCount: state.sliceCount + 1,
        };
        lastOutcome = "filled";
        log("slice_filled", {
          sliceId: outcome.sliceId,
          sliceTx: outcome.sliceTxHash,
          swapDeploy: outcome.swapDeployHash,
          sold: state.soldSoFar.toString(),
          bought: state.boughtSoFar.toString(),
        });
      } else if (outcome.status === "skipped") {
        lastOutcome = "skipped";
        log("slice_skipped", { code: outcome.code, message: outcome.message });
        if (outcome.code === "SpendCapExceeded" || outcome.code === "DeadlinePassed") break;
      } else {
        lastOutcome = "paused";
        log("paused", { reason: outcome.reason });
        await vault.pause();
        paused = true;
        break;
      }

      await sleep(cfg.pollIntervalMs);
    }

    if (paused) {
      // Funds remain safe in the vault; settle() can be called after the window
      // closes (by anyone) to return the remainder to the treasury.
      log("agent_stopped_paused", { sold: state.soldSoFar.toString(), bought: state.boughtSoFar.toString() });
    } else {
      log("settling", { sold: state.soldSoFar.toString(), bought: state.boughtSoFar.toString() });
      const settleTx = await vault.settle();
      log("settled", { settleTx });
    }
  } finally {
    await market.close();
  }
}
