import { Link } from "react-router-dom";
import { useDesk } from "../App.js";
import { deriveMetrics } from "../lib/events.js";
import { formatAmount, formatDuration, formatPrice } from "../lib/format.js";
import { useActivity } from "../lib/useActivity.js";
import { CadenceStave } from "../components/CadenceStave.js";
import { Stat } from "../components/ui.js";

export function Overview(): JSX.Element {
  const { config, stream, nowMs } = useDesk();
  const { state, connection } = stream;
  const metrics = deriveMetrics(state, nowMs, config.naiveBaselinePrice);
  const live = connection === "open" && state.status !== "Unknown";
  const activity = useActivity(config.vaultContractHash);

  return (
    <div>
      <section className="hero reveal">
        <span className="eyebrow">Autonomous OTC execution · Casper</span>
        <h1 style={{ marginTop: 14 }}>
          Move size without <span className="accent">moving the market.</span>
        </h1>
        <p className="lede">
          Sign one mandate. An autonomous agent slices the order into a measured cadence of child
          trades and executes them over time, while an on-chain vault enforces every limit it cannot
          exceed.
        </p>
        <div className="controls">
          <Link className="btn" to="/mandate">
            Create a mandate
          </Link>
          <Link className="btn secondary" to="/execution">
            Watch execution
          </Link>
        </div>
      </section>

      <div className="hero-grid">
        <div className="reveal">
          {live ? (
            <>
              <div className="stat-grid" style={{ marginBottom: 20 }}>
                <Stat label="Status" value={state.status} />
                <Stat label="Remaining" value={formatAmount(metrics.remaining, config.sellAsset)} unit={config.sellAsset} />
                <Stat
                  label="Avg price"
                  value={metrics.averagePrice !== null ? formatPrice(metrics.averagePrice) : null}
                />
                <Stat
                  label="Time left"
                  value={metrics.timeLeftMs !== null ? formatDuration(metrics.timeLeftMs) : null}
                />
              </div>
              <CadenceStave state={state} metrics={metrics} sellAsset={config.sellAsset} nowMs={nowMs} compact />
            </>
          ) : (
            <div className="card" style={{ marginBottom: 0 }}>
              <h2>Desk status</h2>
              <p className="sub">
                No mandate is executing right now. Below is the deployed vault's real on-chain
                activity — the live desk takes over automatically the moment fills start streaming.
              </p>
              <div className="stat-grid" style={{ marginTop: 12 }}>
                <Stat label="Network" value={config.chainName} />
                <Stat label="Pair" value={`${config.sellAsset} → ${config.buyAsset}`} />
                <Stat
                  label="Slices executed"
                  value={activity.summary ? String(activity.summary.slices) : null}
                />
                <Stat
                  label="CSPR sold"
                  value={activity.summary ? formatAmount(activity.summary.soldMotes, config.sellAsset) : null}
                  unit={config.sellAsset}
                />
              </div>
              <p className="sub" style={{ marginTop: 14 }}>
                <Link to="/activity">View full on-chain activity →</Link> ·{" "}
                <Link to="/deployments">View deployed contracts →</Link>
              </p>
            </div>
          )}
        </div>

        <div>
          <div className="principle reveal" style={{ marginBottom: 16 }}>
            <span className="eyebrow">The guardrail principle</span>
            <p>
              Spend cap, deadline, slippage, price band, venue and caller are all checked on-chain in
              one entrypoint. Any breach reverts — the contract is the authority, not the agent.
            </p>
          </div>
          <div className="principle reveal" style={{ marginBottom: 16 }}>
            <span className="eyebrow">Plan in the agent, enforce in the contract</span>
            <p>
              The LLM only proposes the next slice. A deterministic executor and the vault validate
              every limit, so a hallucination cannot move funds out of bounds.
            </p>
          </div>
          <div className="principle reveal">
            <span className="eyebrow">Built on Casper</span>
            <p>
              Odra vault · EIP-712 mandates · CSPR.trade routing · x402 premium data · CSPR.cloud
              streaming. Every fill is an explorer link.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
