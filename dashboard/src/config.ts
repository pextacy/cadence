import { networkPreset } from "@cadence/mandate";

/**
 * Runtime configuration, read from Vite env (`VITE_*`). Nothing here fabricates a
 * connection: when streaming is not configured the UI shows an honest "not
 * connected" state rather than sample data.
 */
export interface DashboardConfig {
  streamingUrl?: string;
  apiKey?: string;
  /**
   * Set when a same-origin server proxy injects the CSPR.cloud auth server-side
   * (production: dashboard/server.mjs). Lets streaming be considered configured
   * without the API key ever reaching the browser. In local dev the key is
   * present instead, so either path enables the stream.
   */
  streamProxied: boolean;
  vaultContractHash?: string;
  /** All vaults to show on the portfolio screen (one vault per mandate). */
  vaultContractHashes: string[];
  chainName: string;
  explorerTxBase: string;
  sellAsset: string;
  buyAsset: string;
  /** Optional fixed-point naive-baseline price for the slippage-saved metric. */
  naiveBaselinePrice: bigint | null;
}

function env(key: string): string | undefined {
  const v = import.meta.env[key];
  return typeof v === "string" && v !== "" ? v : undefined;
}

export function loadDashboardConfig(): DashboardConfig {
  const naive = env("VITE_NAIVE_BASELINE_PRICE");
  // VITE_CASPER_NETWORK (mainnet|testnet) selects the chain, streaming endpoint
  // and explorer; each can still be overridden by its explicit VITE_* variable.
  const net = networkPreset(env("VITE_CASPER_NETWORK"));
  const single = env("VITE_VAULT_CONTRACT_HASH");
  const proxied = (env("VITE_STREAM_PROXIED") ?? "").toLowerCase();
  return {
    streamingUrl: env("VITE_CSPR_CLOUD_STREAMING_URL") ?? net.csprCloudStreamingUrl,
    apiKey: env("VITE_CSPR_CLOUD_API_KEY"),
    streamProxied: proxied === "1" || proxied === "true",
    vaultContractHash: single,
    vaultContractHashes: parseHashes(env("VITE_VAULT_CONTRACT_HASHES"), single),
    chainName: env("VITE_CHAIN_NAME") ?? net.chainName,
    explorerTxBase: env("VITE_EXPLORER_TX_BASE") ?? net.explorerTxBase,
    sellAsset: env("VITE_SELL_ASSET") ?? "CSPR",
    buyAsset: env("VITE_BUY_ASSET") ?? "USDC",
    naiveBaselinePrice: naive ? BigInt(naive) : null,
  };
}

/** Credentials present either in the browser (dev) or via a server proxy (prod). */
function hasCredentials(cfg: DashboardConfig): boolean {
  return Boolean(cfg.apiKey || cfg.streamProxied);
}

/** Whether enough is configured to attempt a live stream. */
export function isStreamConfigured(cfg: DashboardConfig): boolean {
  return Boolean(cfg.streamingUrl && hasCredentials(cfg) && cfg.vaultContractHash);
}

/** Whether enough is configured to stream the portfolio (≥ 1 vault). */
export function isPortfolioConfigured(cfg: DashboardConfig): boolean {
  return Boolean(cfg.streamingUrl && hasCredentials(cfg) && cfg.vaultContractHashes.length > 0);
}

/**
 * Resolve the portfolio's vault hashes: a comma-separated list when provided,
 * otherwise the single configured vault, otherwise none. Deduplicated, order
 * preserved.
 */
function parseHashes(csv: string | undefined, single: string | undefined): string[] {
  const raw = csv ? csv.split(",") : single ? [single] : [];
  const out: string[] = [];
  for (const h of raw) {
    const t = h.trim();
    if (t && !out.includes(t)) out.push(t);
  }
  return out;
}
