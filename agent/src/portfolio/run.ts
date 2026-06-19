import { loadConfig } from "../config.js";
import { CsprTradeClient } from "../clients/csprTrade.js";
import { VaultClient } from "../clients/vault.js";
import { Planner } from "../planner/index.js";
import { Executor } from "../executor/index.js";
import {
  evaluateBreaker,
  INITIAL_BREAKER,
  DEFAULT_BREAKER_CONFIG,
  type BreakerSnapshot,
  type SliceOutcomeKind,
} from "../executor/circuit-breaker/breaker.js";
import { realisedVolatilityBps } from "../executor/circuit-breaker/volatility.js";
import { loadSignedMandate, toRuntimeMandate } from "../mandate.js";
import { buildSnapshot, log, sleep, TARGET_SLICES, PRICE_HISTORY_MAX } from "../runtime.js";
import type { MarketSnapshot, VaultState } from "../types.js";
import { Portfolio } from "./manager.js";
import { loadPortfolioManifest } from "./manifest.js";
import type { MandateTrack } from "./types.js";

/** Per-track execution machinery (one Execution Vault per mandate). */
interface TrackRuntime {
  readonly vault: VaultClient;
  readonly executor: Executor;
  breaker: BreakerSnapshot;
  lastOutcome?: SliceOutcomeKind;
}

const DEFAULT_MANIFEST_PATH = "./portfolio.json";

/**
 * Run a portfolio of mandates concurrently. A deterministic scheduler picks the
 * most time-pressured actionable mandate each tick; that mandate takes one slice
 * through its own vault (its own sell/buy pair), then state is recorded and the
 * next tick re-selects. Each mandate keeps its own circuit breaker; realised
 * volatility is tracked per pair (mandates on the same pair share that window).
 *
 * Mirrors the single-mandate loop's safety: the contract is the authority, the
 * breaker pauses fail-safe, and paused tracks are left for later settlement.
 */
export async function runPortfolio(): Promise<void> {
  const cfg = loadConfig();
  const manifest = await loadPortfolioManifest(
    process.env.PORTFOLIO_MANIFEST_PATH ?? DEFAULT_MANIFEST_PATH,
  );

  const market = new CsprTradeClient(cfg.csprTradeMcpUrl, "cspr.trade");
  await market.connect();

  const runtimes = new Map<string, TrackRuntime>();
  const initialTracks: MandateTrack[] = [];

  for (const entry of manifest.mandates) {
    const signed = await loadSignedMandate(entry.signedMandatePath);
    const mandate = toRuntimeMandate(signed.mandate);
    const vault = new VaultClient({
      nodeRpcUrl: cfg.casperNodeRpc,
      chainName: cfg.chainName,
      contractHash: entry.vaultContractHash,
      agentPrivateKeyHex: cfg.agentPrivateKeyHex,
    });
    const executor = new Executor({
      vault,
      market,
      // Gate every state advance on on-chain finality over this track's RPC.
      confirm: vault.confirmationService(),
      sellToken: mandate.sellAsset,
      buyToken: mandate.buyAsset,
      // Proceeds go to this mandate's treasury, never the agent — no commingling,
      // no agent custody. Per-entry override falls back to the configured treasury.
      proceedsRecipient: entry.treasuryAccountHash ?? cfg.treasuryAccountHash,
    });
    runtimes.set(entry.vaultContractHash, { vault, executor, breaker: INITIAL_BREAKER });
    initialTracks.push({
      id: entry.vaultContractHash,
      mandate,
      state: {
        status: "Active",
        soldSoFar: 0n,
        boughtSoFar: 0n,
        sliceCount: 0,
        totalSell: mandate.totalSell,
      },
    });
  }

  let portfolio = new Portfolio(initialTracks);
  // Realised volatility is a property of the market pair, so price samples are
  // pooled per pair (intentional): mandates on the same pair observe the same
  // CSPR>USDC series. The failure-streak half of the breaker stays per track.
  const priceHistory = new Map<string, readonly bigint[]>();
  const planner = new Planner(cfg.llmApiKey, cfg.llmModel);
  // The manifest guarantees ≥ 1 mandate; all tracks share the agent key, so any
  // vault yields the same agent account hash used for x402 payments.
  const firstRuntime = runtimes.get(initialTracks[0]!.id);
  if (firstRuntime === undefined) throw new Error("portfolio has no initialised tracks");
  const agentAccountHash = firstRuntime.vault.agentAccountHash();

  log("portfolio_start", { mandates: manifest.mandates.length });

  try {
    // One captured timestamp per tick: the scheduler, planner and executor all
    // see the same `now`, so selection is deterministic and cannot disagree with
    // the loop guard about whether a track's window is still open.
    for (let nowMs = Date.now(); !portfolio.allDone(nowMs); nowMs = Date.now()) {
      const pick = portfolio.selectNext(nowMs);
      // Unreachable while allDone(nowMs) is false (same predicate); defensive.
      if (pick === null) break;
      const rt = runtimes.get(pick.id);
      if (rt === undefined) throw new Error(`BUG: no runtime for track ${pick.id}`);

      const pair = `${pick.mandate.sellAsset}>${pick.mandate.buyAsset}`;
      const baseSnapshot = await buildSnapshot(
        cfg,
        market,
        agentAccountHash,
        pick.mandate.sellAsset,
        pick.mandate.buyAsset,
      );
      const window = [...(priceHistory.get(pair) ?? []), baseSnapshot.midPrice].slice(-PRICE_HISTORY_MAX);
      priceHistory.set(pair, window);
      const volatilityBps = baseSnapshot.volatilityBps ?? realisedVolatilityBps(window);
      const snapshot: MarketSnapshot =
        volatilityBps === undefined ? baseSnapshot : { ...baseSnapshot, volatilityBps };

      rt.breaker = evaluateBreaker(
        rt.breaker,
        { volatilityBps, lastOutcome: rt.lastOutcome },
        DEFAULT_BREAKER_CONFIG,
      );
      if (rt.breaker.state === "open") {
        log("circuit_breaker_open", { id: pick.id, reason: rt.breaker.reason, volatilityBps });
        await rt.vault.pause();
        portfolio = portfolio.withTrackState(pick.id, { ...pick.state, status: "Paused" });
        continue;
      }

      const proposal = await planner.propose({
        mandate: pick.mandate,
        state: pick.state,
        market: snapshot,
        nowMs,
        targetSlices: TARGET_SLICES,
      });
      log("proposal", {
        id: pick.id,
        sellAmount: proposal.sellAmount.toString(),
        maxSlippageBps: proposal.maxSlippageBps,
        reason: proposal.reason,
      });

      const outcome = await rt.executor.executeOnce(pick.mandate, pick.state, proposal, nowMs);
      if (outcome.status === "filled") {
        rt.lastOutcome = "filled";
        const next: VaultState = {
          ...pick.state,
          soldSoFar: pick.state.soldSoFar + outcome.sellAmount,
          boughtSoFar: pick.state.boughtSoFar + outcome.boughtAmount,
          sliceCount: pick.state.sliceCount + 1,
        };
        portfolio = portfolio.withTrackState(pick.id, next);
        log("slice_filled", {
          id: pick.id,
          sliceId: outcome.sliceId,
          sold: next.soldSoFar.toString(),
          bought: next.boughtSoFar.toString(),
        });
      } else if (outcome.status === "skipped") {
        rt.lastOutcome = "skipped";
        log("slice_skipped", { id: pick.id, code: outcome.code, message: outcome.message });
        // Cap reached / deadline passed: no further progress possible on this
        // track. Mark it complete locally so the scheduler stops selecting it.
        if (outcome.code === "SpendCapExceeded" || outcome.code === "DeadlinePassed") {
          portfolio = portfolio.withTrackState(pick.id, { ...pick.state, status: "Completed" });
        }
      } else {
        rt.lastOutcome = "paused";
        log("paused", { id: pick.id, reason: outcome.reason });
        await rt.vault.pause();
        portfolio = portfolio.withTrackState(pick.id, { ...pick.state, status: "Paused" });
      }

      await sleep(cfg.pollIntervalMs);
    }

    // Settle every track that completed or whose window has closed. Paused tracks
    // are left intact — their funds stay safe in the vault for later settlement.
    const now = Date.now();
    for (const track of portfolio.list()) {
      if (track.state.status === "Paused") {
        log("track_stopped_paused", { id: track.id });
        continue;
      }
      const complete = track.state.soldSoFar >= track.mandate.totalSell;
      const closed = now > track.mandate.endTimeMs;
      if (!complete && !closed) continue;
      const rt = runtimes.get(track.id);
      if (rt === undefined) throw new Error(`BUG: no runtime for track ${track.id}`);
      try {
        // Local state is optimistic; the vault is the authority and reverts (caught
        // below) if it is not actually settleable yet.
        const settleTx = await rt.vault.settle();
        log("settled", { id: track.id, settleTx });
      } catch (err) {
        log("settle_skipped", { id: track.id, reason: err instanceof Error ? err.message : String(err) });
      }
    }
  } finally {
    await market.close();
  }
}
