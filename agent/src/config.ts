import "dotenv/config";
import { z } from "zod";
import { networkPreset } from "@cadence/mandate";

/**
 * Agent configuration. Every value comes from the environment — there are no
 * defaults that fake a service. Optional values are only those the agent can run
 * without (e.g. premium-data resource URL).
 */
const ConfigSchema = z.object({
  casperNodeRpc: z.string().url(),
  chainName: z.string().min(1),
  csprCloudApiKey: z.string().min(1),
  csprCloudRestUrl: z.string().url(),
  csprCloudStreamingUrl: z.string().min(1),
  csprTradeMcpUrl: z.string().url(),
  casperMcpUrl: z.string().url().optional(),
  x402FacilitatorUrl: z.string().url().optional(),
  x402DepthResource: z.string().url().optional(),
  agentPrivateKeyHex: z.string().regex(/^(0x)?[0-9a-fA-F]{64}$/),
  llmApiKey: z.string().min(1),
  llmModel: z.string().min(1),
  vaultContractHash: z.string().min(1),
  vaultPackageHash: z.string().min(1).optional(),
  sellAsset: z.string().min(1),
  buyAsset: z.string().min(1),
  buyAssetContractHash: z.string().optional(),
  pollIntervalMs: z.number().int().positive(),
});

export type Config = z.infer<typeof ConfigSchema>;

function required(name: string): string {
  const v = process.env[name];
  if (v === undefined || v === "") {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return v;
}

function optional(name: string): string | undefined {
  const v = process.env[name];
  return v === undefined || v === "" ? undefined : v;
}

/** Load and validate configuration from the environment. Throws on any gap. */
export function loadConfig(): Config {
  // CASPER_NETWORK (mainnet|testnet) selects the chain + CSPR.cloud endpoints;
  // any individual URL below can still be overridden by its explicit variable.
  const net = networkPreset(process.env.CASPER_NETWORK);
  const raw = {
    casperNodeRpc: process.env.CASPER_NODE_RPC ?? net.nodeRpcUrl,
    chainName: process.env.CASPER_CHAIN_NAME ?? net.chainName,
    csprCloudApiKey: required("CSPR_CLOUD_API_KEY"),
    csprCloudRestUrl: process.env.CSPR_CLOUD_REST_URL ?? net.csprCloudRestUrl,
    csprCloudStreamingUrl: process.env.CSPR_CLOUD_STREAMING_URL ?? net.csprCloudStreamingUrl,
    csprTradeMcpUrl: process.env.CSPR_TRADE_MCP_URL ?? "https://mcp.cspr.trade",
    casperMcpUrl: optional("CASPER_MCP_URL"),
    x402FacilitatorUrl: optional("X402_FACILITATOR_URL"),
    x402DepthResource: optional("X402_DEPTH_RESOURCE_URL"),
    agentPrivateKeyHex: required("AGENT_PRIVATE_KEY"),
    llmApiKey: required("LLM_API_KEY"),
    llmModel: process.env.LLM_MODEL ?? "claude-sonnet-4-6",
    vaultContractHash: required("VAULT_CONTRACT_HASH"),
    vaultPackageHash: optional("VAULT_PACKAGE_HASH"),
    sellAsset: process.env.SELL_ASSET ?? "CSPR",
    buyAsset: required("BUY_ASSET"),
    buyAssetContractHash: optional("BUY_ASSET_CONTRACT_HASH"),
    pollIntervalMs: Number(process.env.POLL_INTERVAL_MS ?? "15000"),
  };
  return ConfigSchema.parse(raw);
}
