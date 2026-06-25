import { useDesk } from "../App.js";
import { deriveMetrics } from "../lib/events.js";
import { formatAmount, formatBps, formatDuration, formatPrice, shortHash } from "../lib/format.js";
import { useActivity } from "../lib/useActivity.js";
import { CadenceStave } from "../components/CadenceStave.js";
import { NonDataState, SliceBadge, Stat, StatusBadge } from "../components/ui.js";

export function LiveExecution(): JSX.Element {
  const { config, stream, nowMs } = useDesk();
  const { state, connection, applied, lastError } = stream;
  const activity = useActivity(config.vaultContractHash);

  const head = (
    <div className="page-head">
      <span className="eyebrow">02 · Live execution</span>
      <h1>What the desk is doing now</h1>
      <p className="lede">
        Every figure and beat below is reconstructed from the vault's on-chain events. Nothing is
        sampled.
      </p>
    </div>
  );

  if (connection === "idle") {
    return (
      <div>
        {head}
        <NonDataState kind="empty" title="Not connected to live state">
          Set <span className="mono">VITE_CSPR_CLOUD_STREAMING_URL</span>,{" "}
          <span className="mono">VITE_CSPR_CLOUD_API_KEY</span> and{" "}
          <span className="mono">VITE_VAULT_CONTRACT_HASH</span> to stream the vault's events.
        </NonDataState>
      </div>
    );
  }
  if (connection === "error") {
    return (
      <div>
        {head}
        <NonDataState kind="error" title="Streaming connection error">
          {lastError ?? "Reconnecting…"}
        </NonDataState>
      </div>
    );
  }
  if (connection === "connecting") {
    return (
      <div>
        {head}
        <NonDataState kind="loading" title="Connecting to CSPR.cloud streaming…" />
      </div>
    );
  }
  if (applied === 0) {
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
            New events appear here the instant they land. Meanwhile, this is the vault's execution
            so far, read from on-chain activity.
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

  const metrics = deriveMetrics(state, nowMs, config.naiveBaselinePrice);
  const sells = config.sellAsset;
  const buys = config.buyAsset;

  return (
    <div>
      {head}

      <div className="card reveal" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <div>
          <span className="eyebrow">Mandate</span>
          <h2 style={{ marginTop: 6 }}>
            {sells} → {buys}
          </h2>
        </div>
        <StatusBadge status={state.status} />
      </div>

      <div className="stat-grid reveal" style={{ marginBottom: 20 }}>
        <Stat label="Remaining size" value={formatAmount(metrics.remaining, sells)} unit={sells} />
        <Stat label="Average price" value={metrics.averagePrice !== null ? formatPrice(metrics.averagePrice) : null} unit={`${buys}/${sells}`} />
        <Stat label="Slippage saved" value={metrics.slippageSavedBps !== null ? formatBps(metrics.slippageSavedBps) : null} />
        <Stat label="Time left" value={metrics.timeLeftMs !== null ? formatDuration(metrics.timeLeftMs) : null} />
      </div>

      <div className="reveal" style={{ marginBottom: 20 }}>
        <CadenceStave state={state} metrics={metrics} sellAsset={sells} nowMs={nowMs} />
      </div>

      {config.naiveBaselinePrice === null && (
        <div className="warn">
          Set <span className="mono">VITE_NAIVE_BASELINE_PRICE</span> — the price a single naive sell
          would realise on the same pool — to show slippage saved.
        </div>
      )}

      <div className="card reveal">
        <h2>Slice feed</h2>
        <p className="sub">Every child order, its price, the agent's reason, and an explorer link.</p>
        {state.slices.length === 0 ? (
          <NonDataState kind="empty" title="No slices executed yet" />
        ) : (
          <table className="feed">
            <thead>
              <tr>
                <th>#</th>
                <th>Status</th>
                <th>Sell</th>
                <th>Bought</th>
                <th>Min out</th>
                <th>Venue</th>
                <th>Reason</th>
                <th>Swap</th>
              </tr>
            </thead>
            <tbody>
              {[...state.slices].reverse().map((s) => (
                <tr key={s.sliceId}>
                  <td className="num">{s.sliceId}</td>
                  <td><SliceBadge status={s.status} /></td>
                  <td className="num">{formatAmount(s.sellAmount, sells)}</td>
                  <td className="num">{s.boughtAmount !== undefined ? formatAmount(s.boughtAmount, buys) : "—"}</td>
                  <td className="num">{formatAmount(s.minOut, buys)}</td>
                  <td>{s.venue}</td>
                  <td className="reason">{s.reason ?? "—"}</td>
                  <td>
                    {s.swapDeployHash ? (
                      <a href={`${config.explorerTxBase}${s.swapDeployHash}`} target="_blank" rel="noreferrer">
                        {shortHash(s.swapDeployHash)}
                      </a>
                    ) : (
                      "—"
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
