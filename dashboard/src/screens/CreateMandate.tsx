import { useMemo, useState } from "react";
import {
  addressFromPrivateKey,
  buildMandateDomain,
  humanSummary,
  signMandate,
  toBaseUnits,
  type Mandate,
  type SignedMandate,
} from "@cadence/mandate";

/** Placeholder treasury shown before a signing key is entered; signMandate binds
 *  the real signer address on sign, so this never reaches a signed mandate. */
const UNSET_TREASURY = "0x" + "00".repeat(20);

const KEY_RE = /^(0x)?[0-9a-fA-F]{64}$/;
import type { DashboardConfig } from "../config.js";

interface Form {
  sellAsset: string;
  buyAsset: string;
  totalSell: string; // human units
  startLocal: string; // datetime-local
  endLocal: string;
  slippagePct: string;
  priceFloor: string;
  priceCeiling: string;
  strategy: "TWAP" | "VWAP" | "ADAPTIVE";
  venue: string;
  venueAddresses: string;
  devKey: string;
}

function toFixedPrice(human: string): string {
  if (!human.trim()) return "0";
  const [whole, frac = ""] = human.trim().split(".");
  const fracPadded = (frac + "0".repeat(9)).slice(0, 9);
  return BigInt((whole || "0") + fracPadded).toString();
}

const now = new Date();
const plus = (h: number) => new Date(now.getTime() + h * 3_600_000).toISOString().slice(0, 16);

const DEFAULTS: Form = {
  sellAsset: "CSPR",
  buyAsset: "WUSDC",
  totalSell: "100",
  startLocal: plus(0),
  endLocal: plus(72),
  slippagePct: "1.00",
  priceFloor: "",
  priceCeiling: "",
  strategy: "TWAP",
  venue: "cspr.trade",
  venueAddresses: "hash-6c2bce9b90acb75238b640758b99904b6ff1fc243e765722397e045ac76b8dcb",
  devKey: "",
};

export function CreateMandate({ config }: { config: DashboardConfig }): JSX.Element {
  const [form, setForm] = useState<Form>(DEFAULTS);
  const [signed, setSigned] = useState<SignedMandate | null>(null);
  const [signError, setSignError] = useState<string | null>(null);

  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm((f) => ({ ...f, [k]: v }));

  const { mandate, errors } = useMemo(() => buildMandate(form), [form]);

  const summary = mandate ? humanSummary(mandate) : null;
  const canSign = mandate !== null && KEY_RE.test(form.devKey);

  function onSign() {
    setSignError(null);
    if (!mandate) return;
    try {
      const domain = buildMandateDomain(config.chainName);
      const result = signMandate(mandate, domain, form.devKey);
      setSigned(result);
    } catch (err) {
      setSignError(err instanceof Error ? err.message : String(err));
    }
  }

  function download() {
    if (!signed) return;
    const blob = new Blob([JSON.stringify(signed, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "mandate.signed.json";
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div>
      <div className="page-head">
        <span className="eyebrow">01 · Mandate</span>
        <h1>Authorise the whole execution once</h1>
        <p className="lede">
          One signed mandate sets the limits the agent works inside. No per-trade approvals, no gas
          to sign — and the vault enforces every field on-chain.
        </p>
      </div>
      <div className="card reveal">
        <h2>Order</h2>
        <p className="sub">What to sell, what to buy, and how much.</p>

        <div className="row2">
          <div className="field">
            <label htmlFor="sell">Sell asset</label>
            <input id="sell" value={form.sellAsset} onChange={(e) => set("sellAsset", e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="buy">Buy asset</label>
            <input id="buy" value={form.buyAsset} onChange={(e) => set("buyAsset", e.target.value)} />
          </div>
        </div>

        <div className="field">
          <label htmlFor="amount">Total size ({form.sellAsset})</label>
          <input id="amount" inputMode="decimal" value={form.totalSell} onChange={(e) => set("totalSell", e.target.value)} />
          {errors.totalSell && <div className="error">{errors.totalSell}</div>}
        </div>

        <div className="row2">
          <div className="field">
            <label htmlFor="start">Window start</label>
            <input id="start" type="datetime-local" value={form.startLocal} onChange={(e) => set("startLocal", e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="end">Window end</label>
            <input id="end" type="datetime-local" value={form.endLocal} onChange={(e) => set("endLocal", e.target.value)} />
            {errors.window && <div className="error">{errors.window}</div>}
          </div>
        </div>

        <div className="row2">
          <div className="field">
            <label htmlFor="slip">Max slippage (%)</label>
            <input id="slip" inputMode="decimal" value={form.slippagePct} onChange={(e) => set("slippagePct", e.target.value)} />
            {errors.slippage && <div className="error">{errors.slippage}</div>}
          </div>
          <div className="field">
            <label htmlFor="strategy">Strategy</label>
            <select id="strategy" value={form.strategy} onChange={(e) => set("strategy", e.target.value as Form["strategy"])}>
              <option value="TWAP">TWAP (time-weighted)</option>
              <option value="VWAP">VWAP (volume-weighted)</option>
              <option value="ADAPTIVE">Adaptive (volatility-aware)</option>
            </select>
          </div>
        </div>

        <div className="row2">
          <div className="field">
            <label htmlFor="floor">Price floor ({form.buyAsset}/{form.sellAsset}, optional)</label>
            <input id="floor" inputMode="decimal" value={form.priceFloor} onChange={(e) => set("priceFloor", e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="ceil">Price ceiling (optional)</label>
            <input id="ceil" inputMode="decimal" value={form.priceCeiling} onChange={(e) => set("priceCeiling", e.target.value)} />
          </div>
        </div>

        <div className="field">
          <label htmlFor="venue">Venue allowlist</label>
          <input id="venue" value={form.venue} onChange={(e) => set("venue", e.target.value)} />
          <div className="hint">Comma-separated. The vault rejects any swap to a venue not on this list.</div>
        </div>

        <div className="field">
          <label htmlFor="venueAddresses">Venue addresses</label>
          <input
            id="venueAddresses"
            value={form.venueAddresses}
            onChange={(e) => set("venueAddresses", e.target.value)}
          />
          <div className="hint">
            Comma-separated, one Casper address per venue above. The vault releases each
            slice only to these addresses — the agent cannot redirect funds elsewhere.
          </div>
          {errors.venue && <div className="error">{errors.venue}</div>}
        </div>
      </div>

      <div className="card reveal">
        <h2>What you are authorising</h2>
        {summary ? <div className="summary">{summary}</div> : <div className="error">Fix the fields above to preview the mandate.</div>}

        <div className="warn">
          Development signer: paste a secp256k1 private key to sign locally. In production the
          treasurer signs with their wallet (CSPR.click) — the key never leaves it.
        </div>
        <div className="field">
          <label htmlFor="devkey">Signing key (secp256k1, 32-byte hex)</label>
          <input id="devkey" type="password" value={form.devKey} placeholder="0x…" onChange={(e) => set("devKey", e.target.value)} />
        </div>

        <div className="controls">
          <button className="btn" disabled={!canSign} onClick={onSign}>Sign mandate</button>
          {signed && <button className="btn secondary" onClick={download}>Download signed mandate</button>}
        </div>
        {signError && <div className="error" style={{ marginTop: 10 }}>{signError}</div>}

        {signed && (
          <div style={{ marginTop: 16 }}>
            <div className="field">
              <label>EIP-712 digest</label>
              <div className="codeblock">{signed.digest}</div>
            </div>
            <div className="field">
              <label>Signature</label>
              <div className="codeblock">{signed.signature}</div>
            </div>
            <div className="field">
              <label>Recovered signer</label>
              <div className="codeblock">{signed.signer}</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

interface BuildResult {
  mandate: Mandate | null;
  errors: { totalSell?: string; window?: string; slippage?: string; venue?: string };
}

function buildMandate(form: Form): BuildResult {
  const errors: BuildResult["errors"] = {};
  let total = 0n;
  try {
    total = toBaseUnits(form.totalSell, form.sellAsset);
    if (total <= 0n) errors.totalSell = "Total size must be greater than zero.";
  } catch {
    errors.totalSell = "Enter a valid number.";
  }

  const start = Math.floor(new Date(form.startLocal).getTime() / 1000);
  const end = Math.floor(new Date(form.endLocal).getTime() / 1000);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) {
    errors.window = "Window end must be after the start.";
  }

  const slipPct = Number(form.slippagePct);
  if (!Number.isFinite(slipPct) || slipPct < 0 || slipPct > 100) {
    errors.slippage = "Slippage must be between 0 and 100%.";
  }

  const venueAllowlist = form.venue.split(",").map((v) => v.trim()).filter(Boolean);
  const venueAddresses = form.venueAddresses.split(",").map((v) => v.trim()).filter(Boolean);
  if (venueAllowlist.length === 0) {
    errors.venue = "Add at least one venue.";
  } else if (venueAddresses.length !== venueAllowlist.length) {
    errors.venue = `Provide one address per venue (${venueAllowlist.length} venue(s), ${venueAddresses.length} address(es)).`;
  }

  if (Object.keys(errors).length > 0) return { mandate: null, errors };

  // Derive the treasury from the signing key when present so the preview and
  // EIP-712 digest match what will actually be signed; signMandate re-binds it.
  const treasury = KEY_RE.test(form.devKey) ? addressFromPrivateKey(form.devKey) : UNSET_TREASURY;

  const mandate: Mandate = {
    version: 1,
    treasury,
    sellAsset: form.sellAsset,
    buyAsset: form.buyAsset,
    totalSellAmount: total.toString(),
    startTime: start,
    endTime: end,
    maxSlippageBps: Math.round(slipPct * 100),
    priceFloor: toFixedPrice(form.priceFloor),
    priceCeiling: toFixedPrice(form.priceCeiling),
    strategy: form.strategy,
    venueAllowlist,
    venueAddresses,
    nonce: randomNonce(),
  };
  return { mandate, errors };
}

function randomNonce(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return "0x" + Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}
