/**
 * Casper network presets — the single source of truth for the per-network
 * endpoints used by the agent, the scripts and the dashboard. Selecting a network
 * (via `CASPER_NETWORK` / `VITE_CASPER_NETWORK`) picks the chain name and the
 * matching CSPR.cloud + explorer URLs; any individual URL can still be overridden
 * by its explicit environment variable. Pure data + helpers — safe in the browser.
 */

export type CasperNetwork = "mainnet" | "testnet";

export interface NetworkPreset {
  /** Casper chain name used in transactions and the EIP-712 domain. */
  chainName: string;
  /** JSON-RPC node endpoint. */
  nodeRpcUrl: string;
  /** CSPR.cloud REST base URL. */
  csprCloudRestUrl: string;
  /** CSPR.cloud streaming (WebSocket) base URL. */
  csprCloudStreamingUrl: string;
  /** cspr.live explorer base for a deploy/transaction hash. */
  explorerTxBase: string;
}

export const NETWORK_PRESETS: Record<CasperNetwork, NetworkPreset> = {
  mainnet: {
    chainName: "casper",
    nodeRpcUrl: "https://node.mainnet.cspr.cloud/rpc",
    csprCloudRestUrl: "https://api.cspr.cloud",
    csprCloudStreamingUrl: "wss://streaming.cspr.cloud",
    explorerTxBase: "https://cspr.live/deploy/",
  },
  testnet: {
    chainName: "casper-test",
    nodeRpcUrl: "https://node.testnet.cspr.cloud/rpc",
    csprCloudRestUrl: "https://api.testnet.cspr.cloud",
    csprCloudStreamingUrl: "wss://streaming.testnet.cspr.cloud",
    explorerTxBase: "https://testnet.cspr.live/deploy/",
  },
};

/**
 * Normalise a network selector to a {@link CasperNetwork}. Accepts the friendly
 * names (`mainnet`/`testnet`) and the chain names (`casper`/`casper-test`).
 * Defaults to `testnet` when unset. Throws on an unrecognised value.
 */
export function resolveNetwork(value: string | undefined): CasperNetwork {
  const v = (value ?? "testnet").trim().toLowerCase();
  if (v === "mainnet" || v === "casper") return "mainnet";
  if (v === "testnet" || v === "casper-test" || v === "") return "testnet";
  throw new Error(`Unknown Casper network "${value}" (expected "mainnet" or "testnet")`);
}

/** The endpoint preset for a network selector. */
export function networkPreset(value: string | undefined): NetworkPreset {
  return NETWORK_PRESETS[resolveNetwork(value)];
}
