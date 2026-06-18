import { useDesk } from "../App.js";
import { usePortfolioStream } from "../lib/useVaultStream.js";
import { aggregatePortfolio } from "../lib/portfolio.js";
import { deriveMetrics } from "../lib/events.js";
import { formatAmount, formatDuration, formatPrice, shortHash } from "../lib/format.js";
import { NonDataState, Stat, StatusBadge } from "../components/ui.js";

export function Portfolio(): JSX.Element {
  const { config, nowMs } = useDesk();
  const { vaults, connection } = usePortfolioStream(config);

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
    return (
      <div>
        {head}
        <NonDataState
          kind="loading"
          title={
            connection === "connecting"
              ? "Connecting to CSPR.cloud streaming…"
              : "Connected. Waiting for the first vault event…"
          }
        />
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
              return (
                <tr key={v.id}>
                  <td className="mono">{shortHash(v.id)}</td>
                  <td>
                    <StatusBadge status={v.state.status} />
                  </td>
                  <td className="num">{formatAmount(m.remaining, sells)}</td>
                  <td className="num">{formatAmount(v.state.soldSoFar, sells)}</td>
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
