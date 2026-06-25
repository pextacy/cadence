import { useDesk } from "../App.js";
import { shortHash } from "../lib/format.js";

/** The live testnet deployment. Real, on-chain, verifiable on the explorer — this
 * page reflects the backend's deployed contracts regardless of the event stream. */
const ADAPTER_PACKAGE = "6c2bce9b90acb75238b640758b99904b6ff1fc243e765722397e045ac76b8dcb";
const TXS: Array<{ label: string; hash: string }> = [
  { label: "Vault install (signature verified on-chain)", hash: "692a3c1f4d6b17c2d5a77d79b01e7961dc40e53fe433064b078dae5397d3d768" },
  { label: "Vault funded (100 CSPR)", hash: "3d77823749fc3773cb6862da308426488733c19ef4ecea6ddfc2676f06a52c49" },
  { label: "execute_slice → atomic swap → fill", hash: "d3902a11c6503da231fdacd9a073493dfabe1599c3f90736c29c5c98fb8f3594" },
];

function pkgUrl(explorerTxBase: string, pkg: string): string {
  // explorerTxBase is ".../deploy/"; swap to the contract-package path.
  return explorerTxBase.replace(/\/deploy\/?$/, "/contract-package/") + pkg;
}

export function Deployments(): JSX.Element {
  const { config } = useDesk();
  const base = config.explorerTxBase;
  const vaultPkg = (config.vaultContractHash ?? "").replace(/^(hash-|contract-package-)/, "");

  return (
    <div>
      <div className="page-head">
        <span className="eyebrow">On-chain · Deployments</span>
        <h1>Live contracts</h1>
        <p className="lede">
          Every component of the desk, deployed and verifiable on Casper {config.chainName}. The
          vault enforces the mandate; the adapter settles slices atomically.
        </p>
      </div>

      <div className="card reveal">
        <h2>Contracts</h2>
        <table className="feed" style={{ marginTop: 8 }}>
          <tbody>
            <tr>
              <td>Execution Vault (package)</td>
              <td className="num">
                {vaultPkg ? (
                  <a href={pkgUrl(base, vaultPkg)} target="_blank" rel="noreferrer" className="mono">
                    {shortHash(vaultPkg)}
                  </a>
                ) : (
                  "—"
                )}
              </td>
            </tr>
            <tr>
              <td>Cep18 Swap Adapter (venue)</td>
              <td className="num">
                <a href={pkgUrl(base, ADAPTER_PACKAGE)} target="_blank" rel="noreferrer" className="mono">
                  {shortHash(ADAPTER_PACKAGE)}
                </a>
              </td>
            </tr>
            <tr>
              <td>Network</td>
              <td className="num">{config.chainName}</td>
            </tr>
            <tr>
              <td>Pair</td>
              <td className="num">{config.sellAsset} → {config.buyAsset}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <div className="card reveal" style={{ marginTop: 20 }}>
        <h2>Key transactions</h2>
        <table className="feed" style={{ marginTop: 8 }}>
          <tbody>
            {TXS.map((t) => (
              <tr key={t.hash}>
                <td>{t.label}</td>
                <td className="num">
                  <a href={`${base}${t.hash}`} target="_blank" rel="noreferrer" className="mono">
                    {shortHash(t.hash)}
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="sub" style={{ marginTop: 14 }}>
          All transactions are real and finalized on Casper {config.chainName}. Click any hash to
          open it on the explorer.
        </p>
      </div>
    </div>
  );
}
