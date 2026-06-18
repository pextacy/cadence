import { loadConfig, type Config } from "./config.js";
import { CsprTradeClient } from "./clients/csprTrade.js";
import { VaultClient } from "./clients/vault.js";
import { fetchWithX402 } from "./clients/x402.js";
import { Planner } from "./planner/index.js";
import { Executor } from "./executor/index.js";
import { loadSignedMandate, toRuntimeMandate } from "./mandate.js";
import { priceFixed } from "./units.js";
import type { MarketSnapshot, RuntimeMandate, VaultState } from "./types.js";

const REFERENCE_QUOTE_AMOUNT = 1_000_000n;
const TARGET_SLICES = 10;

/** A structured log line the dashboard/operator can follow. */
function log(event: string, detail: Record<string, unknown> = {}): void {
  console.log(JSON.stringify({ ts: new Date().toISOString(), event, ...detail }));
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Build a market snapshot, paying for premium depth/volatility via x402 if set. */
async function buildSnapshot(
  cfg: Config,
  market: CsprTradeClient,
  agentAccountHash: string,
): Promise<MarketSnapshot> {
  const ref = await market.getQuote({
    tokenIn: cfg.sellAsset,
    tokenOut: cfg.buyAsset,
    amount: REFERENCE_QUOTE_AMOUNT,
  });
  const midPrice = priceFixed(ref.quotedOut, REFERENCE_QUOTE_AMOUNT);
  const snapshot: MarketSnapshot = { midPrice, takenAtMs: Date.now() };

  if (cfg.x402DepthResource) {
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
      if (typeof data.volatility_bps === "number") snapshot.volatilityBps = data.volatility_bps;
      if (data.depth_sell) snapshot.depthSell = BigInt(data.depth_sell);
      log("x402_payment", { resource: proof.resource, amount: proof.amount, asset: proof.asset });
    } catch (err) {
      log("x402_skipped", { reason: err instanceof Error ? err.message : String(err) });
    }
  }
  return snapshot;
}

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
    sellToken: cfg.sellAsset,
    buyToken: cfg.buyAsset,
    // Non-custodial: swap proceeds settle directly to the treasury, never the
    // agent. The agent holds no treasury funds (see CLAUDE.md §4.1).
    proceedsRecipient: cfg.treasuryAccountHash,
  });

  // The contract is the authority on state; the agent tracks its own submissions
  // and the dashboard reconstructs authoritative state from on-chain events.
  const state: VaultState = {
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
  });

  try {
    while (state.soldSoFar < mandate.totalSell && Date.now() <= mandate.endTimeMs) {
      const snapshot = await buildSnapshot(cfg, market, agentAccountHash);
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
        state.soldSoFar += outcome.sellAmount;
        state.boughtSoFar += outcome.boughtAmount;
        state.sliceCount += 1;
        log("slice_filled", {
          sliceId: outcome.sliceId,
          sliceTx: outcome.sliceTxHash,
          swapDeploy: outcome.swapDeployHash,
          sold: state.soldSoFar.toString(),
          bought: state.boughtSoFar.toString(),
        });
      } else if (outcome.status === "skipped") {
        log("slice_skipped", { code: outcome.code, message: outcome.message });
        if (outcome.code === "SpendCapExceeded" || outcome.code === "DeadlinePassed") break;
      } else {
        log("paused", { reason: outcome.reason });
        await vault.pause();
        break;
      }

      await sleep(cfg.pollIntervalMs);
    }

    // Settlement is only valid once the cap is reached or the window has closed;
    // the contract reverts otherwise. After a circuit-breaker pause (neither
    // condition met) leave the vault for the treasury to resume or settle.
    const completed = state.soldSoFar >= mandate.totalSell;
    const windowClosed = Date.now() > mandate.endTimeMs;
    if (completed || windowClosed) {
      log("settling", { sold: state.soldSoFar.toString(), bought: state.boughtSoFar.toString() });
      const settleTx = await vault.settle();
      log("settled", { settleTx });
    } else {
      log("settle_skipped", {
        reason: "vault paused before completion/deadline; left for treasury",
        sold: state.soldSoFar.toString(),
      });
    }
  } finally {
    await market.close();
  }
}
