import { useState } from "react";
import { useDesk } from "../App.js";
import { askPlanner, type PlanResult } from "../lib/plan.js";
import { formatAmount, formatBps } from "../lib/format.js";
import { NonDataState, Stat } from "../components/ui.js";

const DEFAULT_TOTAL = 100_000_000_000n; // 100 CSPR, the deployed demo mandate
const DEFAULT_SLIPPAGE_BPS = 100; // 1%
const TARGET_SLICES = 10;

/**
 * Live LLM commentary: ask the Gemini planner what the next slice should be for the
 * current vault state, and show its reasoning. This is the same model + prompt the
 * autonomous agent uses — a window into the AI's decision, gated by the contract.
 */
export function AIPlanner(): JSX.Element {
  const { config, stream } = useDesk();
  const { state } = stream;

  const [result, setResult] = useState<PlanResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const totalSell = state.totalSell > 0n ? state.totalSell : DEFAULT_TOTAL;
  const soldSoFar = state.soldSoFar;
  const maxSlippageBps = state.maxSlippageBps ?? DEFAULT_SLIPPAGE_BPS;
  const slicesRemaining = Math.max(1, TARGET_SLICES - state.slices.length);

  const run = async (): Promise<void> => {
    setLoading(true);
    setError(null);
    try {
      const r = await askPlanner({
        sellAsset: config.sellAsset,
        buyAsset: config.buyAsset,
        totalSell,
        soldSoFar,
        maxSlippageBps,
        strategy: "TWAP",
        slicesRemaining,
      });
      setResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div>
      <div className="page-head">
        <span className="eyebrow">AI · Planner</span>
        <h1>What the agent is thinking</h1>
        <p className="lede">
          The Gemini planner proposes the next slice from the mandate and live state. It only
          proposes — the deterministic executor and the on-chain vault enforce every limit, so a
          model error can never breach the mandate.
        </p>
      </div>

      <div className="stat-grid reveal" style={{ marginBottom: 20 }}>
        <Stat label="Total to sell" value={formatAmount(totalSell, config.sellAsset)} unit={config.sellAsset} />
        <Stat label="Sold so far" value={formatAmount(soldSoFar, config.sellAsset)} unit={config.sellAsset} />
        <Stat label="Slippage cap" value={formatBps(maxSlippageBps)} />
        <Stat label="Slices left" value={String(slicesRemaining)} />
      </div>

      <div className="card reveal">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h2 style={{ margin: 0 }}>Planner proposal</h2>
          <button className="btn" onClick={run} disabled={loading}>
            {loading ? "Thinking…" : "Ask the planner"}
          </button>
        </div>

        {error && (
          <div style={{ marginTop: 16 }}>
            <NonDataState kind="error" title="Planner unavailable">
              {error}
            </NonDataState>
          </div>
        )}

        {!result && !error && (
          <p className="sub" style={{ marginTop: 16 }}>
            Press “Ask the planner” to get a live Gemini proposal for the next slice.
          </p>
        )}

        {result && (
          <div style={{ marginTop: 16 }}>
            <div className="stat-grid">
              <Stat label="Proposed slice" value={formatAmount(result.sellAmount, config.sellAsset)} unit={config.sellAsset} />
              <Stat label="Max slippage" value={formatBps(result.maxSlippageBps)} />
            </div>
            <div className="card" style={{ marginTop: 16, background: "var(--surface-2, #f6f6f4)" }}>
              <span className="eyebrow">LLM commentary</span>
              <p style={{ marginTop: 8, fontSize: 16, lineHeight: 1.5 }}>{result.reason}</p>
            </div>
            <p className="sub" style={{ marginTop: 12 }}>
              Model: gemini-2.5-flash · proposal validated against the mandate before any on-chain
              submission.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
