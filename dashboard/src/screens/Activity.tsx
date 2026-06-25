import { useCallback, useEffect, useState } from "react";
import { useDesk } from "../App.js";
import { fetchActivity, type ActivityItem } from "../lib/activity.js";
import { shortHash } from "../lib/format.js";
import { NonDataState } from "../components/ui.js";

/**
 * Historical on-chain activity, reconstructed from CSPR.cloud REST (the deploys
 * that touched the vault). Unlike the live-only event stream, this is always
 * populated — it shows everything that already happened on the vault.
 */
export function Activity(): JSX.Element {
  const { config } = useDesk();
  const [items, setItems] = useState<ActivityItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    if (!config.vaultContractHash) return;
    setLoading(true);
    setError(null);
    try {
      setItems(await fetchActivity(config.vaultContractHash));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [config.vaultContractHash]);

  useEffect(() => {
    void load();
  }, [load]);

  const head = (
    <div className="page-head">
      <span className="eyebrow">On-chain · Activity</span>
      <h1>What has happened</h1>
      <p className="lede">
        Every deploy that touched the vault, newest first — read from CSPR.cloud. The live
        Execution view shows new events as they stream; this is the full history.
      </p>
    </div>
  );

  return (
    <div>
      {head}
      <div className="card reveal">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h2 style={{ margin: 0 }}>Deploy history</h2>
          <button className="btn secondary" onClick={() => void load()} disabled={loading}>
            {loading ? "Loading…" : "Refresh"}
          </button>
        </div>

        {error && (
          <div style={{ marginTop: 16 }}>
            <NonDataState kind="error" title="Could not load activity">
              {error}
            </NonDataState>
          </div>
        )}

        {!error && items && items.length === 0 && (
          <p className="sub" style={{ marginTop: 16 }}>No deploys found for this vault yet.</p>
        )}

        {!error && items && items.length > 0 && (
          <table className="feed" style={{ marginTop: 12 }}>
            <thead>
              <tr>
                <th style={{ textAlign: "left" }}>Action</th>
                <th style={{ textAlign: "left" }}>Detail</th>
                <th>Result</th>
                <th>Deploy</th>
              </tr>
            </thead>
            <tbody>
              {items.map((it) => (
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
        )}
      </div>
    </div>
  );
}
