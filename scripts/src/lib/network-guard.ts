/**
 * Deploy-safety guard. Casper mainnet deploys/funds move real value, so they must
 * never happen by accident: a mainnet target requires an explicit opt-in. This is a
 * pure, side-effect-free check (no env reads, no logging) so it is trivially
 * unit-testable — callers pass the *effective resolved* target and the opt-in flag.
 *
 * The effective target matters because the network is resolved from several
 * independent inputs (`CASPER_NETWORK`, `CASPER_CHAIN_NAME`, `CASPER_NODE_RPC`).
 * A target that is "testnet" by `CASPER_NETWORK` but points its chain name / node
 * RPC at mainnet still hits real mainnet, so the guard inspects all three.
 */

import { NETWORK_PRESETS } from "@cadence/mandate";

/** Network selectors that resolve to Casper mainnet (friendly name + chain name). */
const MAINNET_ALIASES = new Set(["mainnet", "casper"]);

/** Mainnet chain name (e.g. "casper"), derived from the mandate preset. */
const MAINNET_CHAIN_NAME = NETWORK_PRESETS.mainnet.chainName.trim().toLowerCase();

/** Mainnet node RPC host (e.g. "node.mainnet.cspr.cloud"), derived from the preset. */
const MAINNET_RPC_HOST = hostOf(NETWORK_PRESETS.mainnet.nodeRpcUrl);

/** The effective, resolved deploy target — what the deploy/fund will actually hit. */
export interface DeployTarget {
  /** Friendly name or chain name selector (e.g. `CASPER_NETWORK`). */
  network: string;
  /** Effective chain name (e.g. from `networkChainName()`). */
  chainName: string;
  /** Effective JSON-RPC node URL (e.g. from `networkNodeRpc()`). */
  nodeRpc: string;
}

/**
 * Lowercased URL host for a node RPC URL, or `undefined` if it cannot be parsed.
 * Tolerant of values that omit a scheme (e.g. "node.mainnet.cspr.cloud/rpc").
 */
function hostOf(nodeRpc: string): string | undefined {
  const value = nodeRpc.trim();
  if (value === "") return undefined;
  const candidate = /^[a-z][a-z0-9+.-]*:\/\//i.test(value) ? value : `https://${value}`;
  try {
    return new URL(candidate).host.toLowerCase();
  } catch {
    return undefined;
  }
}

/**
 * Whether the effective target resolves to Casper mainnet. True if ANY of:
 * - `network` is a mainnet alias (`mainnet` / `casper`),
 * - the effective `chainName` is the mainnet chain name (`casper`), or
 * - the effective `nodeRpc` host points at the Casper mainnet endpoint.
 */
function isMainnetTarget({ network, chainName, nodeRpc }: DeployTarget): boolean {
  if (MAINNET_ALIASES.has(network.trim().toLowerCase())) return true;
  if (chainName.trim().toLowerCase() === MAINNET_CHAIN_NAME) return true;
  const host = hostOf(nodeRpc);
  return host !== undefined && host === MAINNET_RPC_HOST;
}

/**
 * Throw unless deploying to the given *effective* target is allowed.
 *
 * - Testnet (no mainnet signal on any of network / chainName / nodeRpc) is a no-op.
 * - Mainnet (any mainnet signal, case-insensitive, surrounding whitespace ignored)
 *   throws unless `allowMainnet` is true.
 *
 * Pure: it inspects only its arguments and either returns `void` or throws.
 *
 * @param target effective resolved target — selected network plus the chain name
 *   and node RPC the deploy/fund will actually use.
 * @param allowMainnet explicit opt-in; callers should pass
 *   `process.env.ALLOW_MAINNET === "true"`.
 */
export function assertDeployTargetAllowed(target: DeployTarget, allowMainnet: boolean): void {
  if (!isMainnetTarget(target)) return; // testnet / unknown: no guard.
  if (allowMainnet) return; // opted in.

  throw new Error(
    `Refusing to deploy to Casper mainnet ` +
      `(CASPER_NETWORK="${target.network}", chainName="${target.chainName}", nodeRpc="${target.nodeRpc}"). ` +
      `Mainnet moves real funds. Set ALLOW_MAINNET=true to proceed on mainnet.`,
  );
}
