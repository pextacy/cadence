import { useDesk } from "../App.js";
import { deriveMetrics } from "../lib/events.js";
import { formatAmount, formatBps, formatPrice } from "../lib/format.js";
import { NonDataState, Stat, StatusBadge } from "../components/ui.js";

export function FinalReport(): JSX.Element {
  const { config, stream, nowMs } = useDesk();
  const { state, connection } = stream;

  const head = (
    <div className="page-head">
      <span className="eyebrow">03 · Final report</span>
      <h1>What happened</h1>
      <p className="lede">Reconstructed from the vault's on-chain settlement event.</p>
    </div>
  );

  if (connection === "idle") {
    return (
      <div>
        {head}
        <NonDataState kind="empty" title="Not connected">
          The report is built from the vault's settlement event once streaming is configured.
        </NonDataState>
      </div>
    );
  }
  if (!state.settled) {
    return (
      <div>
        {head}
        <NonDataState kind="empty" title="Not settled yet">
          This appears once the vault emits its settlement event — when the order completes or the
          window closes.
        </NonDataState>
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
          <span className="eyebrow">Outcome</span>
          <h2 style={{ marginTop: 6 }}>{state.settled.completed ? "Order completed" : "Window closed"}</h2>
        </div>
        <StatusBadge status={state.status} />
      </div>

      <div className="stat-grid reveal" style={{ marginBottom: 20 }}>
        <Stat label="Total sold" value={formatAmount(state.soldSoFar, sells)} unit={sells} />
        <Stat label="Total bought" value={formatAmount(state.boughtSoFar, buys)} unit={buys} />
        <Stat label="Average price" value={metrics.averagePrice !== null ? formatPrice(metrics.averagePrice) : null} unit={`${buys}/${sells}`} />
        <Stat label="Slippage saved" value={metrics.slippageSavedBps !== null ? formatBps(metrics.slippageSavedBps) : null} />
      </div>

      <div className="card reveal">
        <h2>Settlement</h2>
        <table className="feed" style={{ marginTop: 8 }}>
          <tbody>
            <tr>
              <td>Outcome</td>
              <td className="num">{state.settled.completed ? "Completed" : "Expired (window closed)"}</td>
            </tr>
            <tr>
              <td>Slices executed</td>
              <td className="num">{state.settled.sliceCount}</td>
            </tr>
            <tr>
              <td>Returned to treasury</td>
              <td className="num">{formatAmount(state.settled.returnedToTreasury, sells)} {sells}</td>
            </tr>
            {state.treasury && (
              <tr>
                <td>Treasury</td>
                <td className="num">{state.treasury}</td>
              </tr>
            )}
          </tbody>
        </table>
        {config.vaultContractHash && (
          <p className="sub" style={{ marginTop: 14 }}>
            Full audit trail on-chain at vault <span className="mono">{config.vaultContractHash}</span>.
          </p>
        )}
      </div>
    </div>
  );
}
