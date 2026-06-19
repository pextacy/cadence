import { join } from "node:path";
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
import { buildSnapshot, log, sleep, submissionDelayMs, TARGET_SLICES, PRICE_HISTORY_MAX } from "./runtime.js";
import type { MarketSnapshot, RuntimeMandate, VaultState } from "./types.js";
import { FileStateStore, defaultStateDir } from "./state/store.js";
import { resolveStartupState } from "./state/reconcile.js";
import { InProcessMetrics, METRICS } from "./observability/metrics.js";
import { FileAuditLog } from "./observability/audit-log.js";
import { HealthServer, type HealthState } from "./observability/health.js";
import { InProcessNonceManager } from "./clients/nonce.js";

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
    // Gate every state advance on on-chain finality over the vault's own RPC.
    confirm: vault.confirmationService(),
    sellToken: mandate.sellAsset,
    buyToken: mandate.buyAsset,
    // Non-custodial: swap proceeds settle directly to the treasury, never the
    // agent. The agent holds no treasury funds (see CLAUDE.md §4.1).
    proceedsRecipient: cfg.treasuryAccountHash,
  });

  // ---- Operational layer (persistence, observability, key serialisation) ----
  // The contract remains the authority on balances; this layer makes the agent
  // crash-recoverable, observable, and safe to restart. The state directory and
  // an optional health port are read from the environment so config validation
  // is unchanged.
  const trackId = cfg.vaultContractHash;
  const stateDir = defaultStateDir();
  const store = new FileStateStore(stateDir);
  const audit = new FileAuditLog(join(stateDir, "audit.jsonl"));
  await audit.init();
  const metrics = new InProcessMetrics();
  const nonce = new InProcessNonceManager(await store.highWaterSeq());

  // Resume operational heuristics (circuit-breaker + price history) from the last
  // snapshot so a restart does not lose volatility context. Authoritative balances
  // stay fresh — on-chain reconciliation is tracked as ROADMAP Wave 6 follow-up.
  const prevSnapshot = await store.loadSnapshot(trackId);

  // The contract is the authority on state. Seed the resume point from the chain
  // rather than assuming a fresh zero state: after a crash/restart mid-order,
  // resetting to zero would re-execute already-submitted slices and desynchronise
  // the agent's slice_id bookkeeping. `resolveStartupState` reads on-chain and
  // fails closed if a read fails while prior progress exists (see reconcile.ts).
  const seeded = await resolveStartupState(
    vault.stateReader(),
    trackId,
    prevSnapshot,
    mandate.totalSell,
  );
  let state: VaultState = seeded.state;

  log("agent_start", {
    totalSell: mandate.totalSell.toString(),
    endTimeMs: mandate.endTimeMs,
    venue: mandate.venueAllowlist,
    strategy: mandate.strategy,
    resumed: prevSnapshot !== null,
    stateSource: seeded.source,
    status: state.status,
    soldSoFar: state.soldSoFar.toString(),
    sliceCount: state.sliceCount,
  });

  // The circuit breaker is consulted before each slice; price history backs the
  // realised-volatility estimate used when premium volatility is not purchased.
  let breaker: BreakerSnapshot = prevSnapshot?.breaker ?? INITIAL_BREAKER;
  let lastOutcome: SliceOutcomeKind | undefined;
  let priceHistory: readonly bigint[] = prevSnapshot?.priceHistory.map((p) => BigInt(p)) ?? [];

  // Health/readiness endpoint (opt-in via HEALTH_PORT) reflects live loop state.
  let draining = false;
  let lastConfirmedSliceMs: number | undefined;
  const healthState = (): HealthState => ({
    loopLive: true,
    ready: true,
    draining,
    ...(lastConfirmedSliceMs !== undefined ? { lastConfirmedSliceMs } : {}),
    breakerState: breaker.state === "open" ? "open" : "closed",
  });
  const healthPort = Number(process.env.HEALTH_PORT ?? "");
  const health =
    Number.isInteger(healthPort) && healthPort > 0
      ? new HealthServer({ port: healthPort, snapshot: healthState, metrics })
      : undefined;
  if (health) await health.start();

  /** Persist the operational snapshot so a crash mid-order recovers cleanly. */
  const persist = async (): Promise<void> => {
    await store.saveSnapshot({
      trackId,
      state,
      breaker,
      priceHistory: priceHistory.map((p) => p.toString()),
      seq: nonce.current(),
      updatedAtMs: Date.now(),
    });
  };

  try {
    // Only trade while the seeded/on-chain status is Active. A vault resumed in a
    // Paused/Completed/Expired state must not busy-spin issuing NotActive skips;
    // it falls through to the settlement decision below.
    while (
      state.status === "Active" &&
      state.soldSoFar < mandate.totalSell &&
      Date.now() <= mandate.endTimeMs
    ) {
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
        metrics.inc(METRICS.breakerTrips);
        await audit.record({
          event: "circuit_breaker_open",
          trackId,
          detail: { reason: breaker.reason ?? null, volatilityBps: volatilityBps ?? null },
          tsMs: Date.now(),
        });
        await vault.pause();
        await persist();
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
      await audit.record({
        event: "proposal",
        trackId,
        detail: {
          sellAmount: proposal.sellAmount.toString(),
          maxSlippageBps: proposal.maxSlippageBps,
          reason: proposal.reason,
        },
        tsMs: Date.now(),
      });

      // Honour the planner's pacing: a strategy spreads child orders across the
      // window via `notBeforeMs`. Wait (bounded by the deadline) until the slice is
      // due before submitting, otherwise the time-weighting is silently lost and
      // every slice fires at the poll cadence. The quote is fetched fresh inside
      // the executor after the wait, so the on-chain commitment is never stale.
      const waitMs = submissionDelayMs(proposal.notBeforeMs, Date.now(), mandate.endTimeMs);
      if (waitMs > 0) {
        log("slice_scheduled", { notBeforeMs: proposal.notBeforeMs, waitMs });
        await sleep(waitMs);
      }

      // Serialise the signed submission through the nonce manager so concurrent
      // tracks never race the agent key (a no-op for a single mandate, but the
      // correct shape for portfolio execution).
      const outcome = await nonce.withSequence(() =>
        executor.executeOnce(mandate, state, proposal, Date.now()),
      );
      if (outcome.status === "filled") {
        state = {
          ...state,
          soldSoFar: state.soldSoFar + outcome.sellAmount,
          boughtSoFar: state.boughtSoFar + outcome.boughtAmount,
          sliceCount: state.sliceCount + 1,
        };
        lastOutcome = "filled";
        lastConfirmedSliceMs = Date.now();
        metrics.inc(METRICS.slicesFilled);
        log("slice_filled", {
          sliceId: outcome.sliceId,
          sliceTx: outcome.sliceTxHash,
          swapDeploy: outcome.swapDeployHash,
          sold: state.soldSoFar.toString(),
          bought: state.boughtSoFar.toString(),
        });
        await audit.record({
          event: "slice_filled",
          trackId,
          sliceId: outcome.sliceId,
          detail: {
            sliceTxHash: outcome.sliceTxHash,
            swapDeployHash: outcome.swapDeployHash,
            sellAmount: outcome.sellAmount.toString(),
            boughtAmount: outcome.boughtAmount.toString(),
            soldSoFar: state.soldSoFar.toString(),
            boughtSoFar: state.boughtSoFar.toString(),
          },
          tsMs: Date.now(),
        });
      } else if (outcome.status === "skipped") {
        lastOutcome = "skipped";
        metrics.inc(METRICS.slicesSkipped);
        log("slice_skipped", { code: outcome.code, message: outcome.message });
        await audit.record({
          event: "slice_skipped",
          trackId,
          detail: { code: outcome.code, message: outcome.message },
          tsMs: Date.now(),
        });
        if (outcome.code === "SpendCapExceeded" || outcome.code === "DeadlinePassed") {
          await persist();
          break;
        }
      } else {
        lastOutcome = "paused";
        metrics.inc(METRICS.slicesPaused);
        log("paused", { reason: outcome.reason });
        await audit.record({
          event: "paused",
          trackId,
          detail: { reason: outcome.reason },
          tsMs: Date.now(),
        });
        await vault.pause();
        await persist();
        break;
      }

      await persist();
      await sleep(cfg.pollIntervalMs);
    }

    // Settlement is only valid once the cap is reached or the window has closed;
    // the contract reverts otherwise. After a circuit-breaker pause (neither
    // condition met) leave the vault for the treasury to resume or settle.
    // Never settle a vault that is not Active: a resumed Completed/Expired vault
    // would revert (or double-settle). Settlement is this run's responsibility
    // only while the vault is still live.
    const completed = state.soldSoFar >= mandate.totalSell;
    const windowClosed = Date.now() > mandate.endTimeMs;
    if (state.status === "Active" && (completed || windowClosed)) {
      log("settling", { sold: state.soldSoFar.toString(), bought: state.boughtSoFar.toString() });
      const settleTx = await vault.settle();
      log("settled", { settleTx });
      await audit.record({
        event: "settled",
        trackId,
        detail: { settleTx, completed, sold: state.soldSoFar.toString() },
        tsMs: Date.now(),
      });
    } else {
      log("settle_skipped", {
        reason: "vault paused before completion/deadline; left for treasury",
        sold: state.soldSoFar.toString(),
      });
    }
  } finally {
    draining = true;
    await persist();
    if (health) await health.stop();
    await audit.flush();
    await store.close();
    await market.close();
  }
}
