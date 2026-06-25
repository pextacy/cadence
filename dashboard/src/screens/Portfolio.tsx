import { useDesk } from "../App.js";
import { usePortfolioStream } from "../lib/useVaultStream.js";
import { aggregatePortfolio } from "../lib/portfolio.js";
import { deriveMetrics } from "../lib/events.js";
import { formatAmount, formatDuration, formatPrice, shortHash } from "../lib/format.js";
import { useActivity } from "../lib/useActivity.js";
import { NonDataState, Stat, StatusBadge } from "../components/ui.js";

export function Portfolio(): JSX.Element {
  const { config, nowMs } = useDesk();
  const { vaults, connection } = usePortfolioStream(config);
  // Streaming is live-only; backfill the pre-stream view with the vault's real
  // on-chain history so the screen is never an empty "waiting" state.
  const activity = useActivity(config.vaultContractHash);

  const head = (
    <div className="page-head">
      <span className="eyebrow">Desk · Portfolio</span>
      <h1>Every mandate at a glance</h1>
      <p className="lede">
        One vault per mandate — funds stay isolated. Each row is reconstructed from that vault's
        on-chain events; nothing is sampled.
      </p>
    </div>
  );

  if (connection === "idle") {
    return (
      <div>
        {head}
        <NonDataState kind="empty" title="No vaults configured">
          Set <span className="mono">VITE_CSPR_CLOUD_STREAMING_URL</span>,{" "}
          <span className="mono">VITE_CSPR_CLOUD_API_KEY</span> and{" "}
          <span className="mono">VITE_VAULT_CONTRACT_HASHES</span> (comma-separated) to stream the
          portfolio.
        </NonDataState>
      </div>
    );
  }
  if (connection === "error") {
    return (
      <div>
        {head}
        <NonDataState kind="error" title="Streaming connection error">
          Reconnecting…
        </NonDataState>
      </div>
    );
  }

  const states = vaults.map((v) => v.state);
  const summary = aggregatePortfolio(states, nowMs);
  const anyData = states.some((s) => s.status !== "Unknown");
  if (!anyData) {
    if (connection === "connecting") {
      return (
        <div>
          {head}
          <NonDataState kind="loading" title="Connecting to CSPR.cloud streaming…" />
        </div>
      );
    }
    const s = activity.summary;
    return (
      <div>
        {head}
        <div className="card reveal">
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div>
              <span className="eyebrow">Live stream connected</span>
              <h2 style={{ marginTop: 6 }}>Waiting for the next on-chain event</h2>
            </div>
            <span className="badge ok"><span className="dot" />Connected</span>
          </div>
          <p className="sub" style={{ marginTop: 8 }}>
            New mandate events appear here the instant they land. Meanwhile, this is the vault's
            execution so far, read from on-chain activity.
          </p>
          <div className="stat-grid" style={{ marginTop: 16 }}>
            <Stat label="Pair" value={`${config.sellAsset} → ${config.buyAsset}`} />
            <Stat label="Slices executed" value={s ? String(s.slices) : null} />
            <Stat label="CSPR sold" value={s ? formatAmount(s.soldMotes, config.sellAsset) : null} unit={config.sellAsset} />
            <Stat label="Funded" value={s ? (s.funded ? "Yes" : "No") : null} />
          </div>
        </div>

        {activity.items && activity.items.length > 0 && (
          <div className="card reveal" style={{ marginTop: 20 }}>
            <h2>On-chain activity</h2>
            <table className="feed" style={{ marginTop: 8 }}>
              <thead>
                <tr>
                  <th style={{ textAlign: "left" }}>Action</th>
                  <th style={{ textAlign: "left" }}>Detail</th>
                  <th>Result</th>
                  <th>Deploy</th>
                </tr>
              </thead>
              <tbody>
                {activity.items.map((it) => (
                  <tr key={it.deployHash}>
                    <td>{it.action}</td>
                    <td className="sub">{it.detail}</td>
                    <td className="num">
                      <span className={`badge ${it.success ? "ok" : "stop"}`}>
                        <span className="dot" />
                        {it.success ? "OK" : "Reverted"}
                      </span>
                    </td>
                    <td className="num">
                      <a href={`${config.explorerTxBase}${it.deployHash}`} target="_blank" rel="noreferrer" className="mono">
                        {shortHash(it.deployHash)}
                      </a>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    );
  }

  const sells = config.sellAsset;
  const buys = config.buyAsset;
  const nearestLeft =
    summary.nearestDeadlineMs !== null ? Math.max(0, summary.nearestDeadlineMs - nowMs) : null;

  return (
    <div>
      {head}

      <div className="stat-grid reveal" style={{ marginBottom: 20 }}>
        <Stat label="Mandates" value={String(summary.mandateCount)} />
        <Stat label="Active" value={String(summary.activeCount)} />
        <Stat label="Remaining" value={formatAmount(summary.totalRemaining, sells)} unit={sells} />
        <Stat
          label="Avg price"
          value={summary.averagePrice !== null ? formatPrice(summary.averagePrice) : null}
          unit={`${buys}/${sells}`}
        />
        <Stat label="Next deadline" value={nearestLeft !== null ? formatDuration(nearestLeft) : null} />
      </div>

      <div className="card reveal">
        <h2>Mandates</h2>
        <p className="sub">
          {summary.activeCount} active · {summary.pausedCount} paused · {summary.completedCount}{" "}
          settled.
        </p>
        <table className="feed">
          <thead>
            <tr>
              <th>Vault</th>
              <th>Pair</th>
              <th>Status</th>
              <th>Remaining</th>
              <th>Sold</th>
              <th>Avg price</th>
              <th>Time left</th>
              <th>Slices</th>
            </tr>
          </thead>
          <tbody>
            {vaults.map((v) => {
              const m = deriveMetrics(v.state, nowMs, config.naiveBaselinePrice);
              // Prefer the vault's own pair (from its MandateInitialised event);
              // fall back to the configured defaults when not yet observed.
              const vSell = v.state.sellAsset ?? sells;
              const vBuy = v.state.buyAsset ?? buys;
              return (
                <tr key={v.id}>
                  <td className="mono">{shortHash(v.id)}</td>
                  <td>{v.state.sellAsset ? `${vSell}/${vBuy}` : "—"}</td>
                  <td>
                    <StatusBadge status={v.state.status} />
                  </td>
                  <td className="num">{formatAmount(m.remaining, vSell)}</td>
                  <td className="num">{formatAmount(v.state.soldSoFar, vSell)}</td>
                  <td className="num">{m.averagePrice !== null ? formatPrice(m.averagePrice) : "—"}</td>
                  <td className="num">{m.timeLeftMs !== null ? formatDuration(m.timeLeftMs) : "—"}</td>
                  <td className="num">{v.state.slices.length}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
