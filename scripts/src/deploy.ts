import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
import type { SignedMandateFile } from "@cadence/agent";

const { Args, CLValue, Key, SessionBuilder } = casper;
import {
  clBytesList,
  clKeyList,
  clStringList,
  hexToBytes,
  loadSecp256k1,
  log,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  requireEnv,
} from "./lib/casper.js";
import { confirmTransaction } from "./lib/confirm.js";
import { assertDeployTargetAllowed } from "./lib/network-guard.js";
import {
  findRecord,
  loadManifest,
  saveManifest,
  upsertRecord,
  type DeploymentRecord,
} from "./lib/manifest.js";

const INSTALL_PAYMENT_MOTES = Number(process.env.DEPLOY_PAYMENT_MOTES ?? "300000000000");
const MANIFEST_PATH = process.env.DEPLOYMENTS_MANIFEST_PATH ?? "./.deployments.json";
const CONFIRM_TIMEOUT_MS = Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000");
const CONFIRM_POLL_INTERVAL_MS = Number(process.env.CONFIRM_POLL_INTERVAL_MS ?? "5000");

export interface DeployResult {
  transactionHash: string;
  contractHash?: string;
  packageHash?: string;
  /** True when an existing confirmed install was reused instead of re-submitted. */
  skipped: boolean;
}

/**
 * Deploy the Execution Vault WASM with the signed mandate's limits as init
 * args. Idempotent + finality-confirmed:
 *
 * 1. If a `confirmed` install for this `(chain, mandateDigest)` already exists
 *    in the manifest, log a skip and return its recorded hashes.
 * 2. Otherwise submit, record `submitted`, poll the tx to finality, then record
 *    `confirmed` / `failed`. Throws on on-chain revert or timeout.
 */
export async function deployVault(): Promise<DeployResult> {
  // Deploy-safety: never install to mainnet without an explicit opt-in.
  assertDeployTargetAllowed(process.env.CASPER_NETWORK ?? "testnet", process.env.ALLOW_MAINNET === "true");

  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const wasmPath = process.env.VAULT_WASM_PATH ?? "../contracts/vault/wasm/ExecutionVault.wasm";
  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  const agentAccountHash = requireEnv("AGENT_ACCOUNT_HASH"); // "account-hash-…"
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const signed = JSON.parse(await readFile(signedPath, "utf8")) as SignedMandateFile;
  const m = signed.mandate;
  const mandateDigest = signed.digest;

  // Idempotency: reuse a prior confirmed install for the same chain + mandate.
  const manifest = await loadManifest(MANIFEST_PATH);
  const existing = findRecord(manifest, { kind: "vault-install", chainName, mandateDigest });
  if (existing?.status === "confirmed") {
    log("vault_deploy_skipped", {
      reason: "already_confirmed",
      chainName,
      mandateDigest,
      transactionHash: existing.transactionHash,
      contractHash: existing.contractHash,
      packageHash: existing.packageHash,
    });
    return {
      transactionHash: existing.transactionHash,
      contractHash: existing.contractHash,
      packageHash: existing.packageHash,
      skipped: true,
    };
  }

  if (m.venueAllowlist.length !== m.venueAddresses.length) {
    throw new Error(
      `Mandate venueAllowlist (${m.venueAllowlist.length}) and venueAddresses ` +
        `(${m.venueAddresses.length}) must be the same length.`,
    );
  }

  const wasm = new Uint8Array(await readFile(wasmPath));

  const args = Args.fromMap({
    agent: CLValue.newCLKey(Key.newKey(agentAccountHash)),
    mandate_digest: clBytesList(hexToBytes(signed.digest)),
    signature: clBytesList(hexToBytes(signed.signature)),
    sell_asset: CLValue.newCLString(m.sellAsset),
    buy_asset: CLValue.newCLString(m.buyAsset),
    total_sell: CLValue.newCLUInt512(m.totalSellAmount),
    end_time_ms: CLValue.newCLUint64(m.endTime * 1000),
    max_slippage_bps: CLValue.newCLUInt32(m.maxSlippageBps),
    price_floor: CLValue.newCLUInt512(m.priceFloor),
    price_ceiling: CLValue.newCLUInt512(m.priceCeiling),
    venues: clStringList(m.venueAllowlist),
    venue_addresses: clKeyList(m.venueAddresses),
  });

  const tx = new SessionBuilder()
    .from(treasuryKey.publicKey)
    .wasm(wasm)
    .installOrUpgrade()
    .runtimeArgs(args)
    .chainName(chainName)
    .payment(INSTALL_PAYMENT_MOTES)
    .build();
  tx.sign(treasuryKey);

  const rpc = makeRpc(nodeRpc);
  const result = await rpc.putTransaction(tx);
  const transactionHash = result.transactionHash.toJSON();

  log("vault_deploy_submitted", { transactionHash, chainName, mandateDigest });

  // Persist the in-flight submission so a crash mid-confirmation is recoverable.
  const submittedRecord: DeploymentRecord = {
    kind: "vault-install",
    chainName,
    mandateDigest,
    transactionHash,
    status: "submitted",
    createdAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(manifest, submittedRecord));

  const outcome = await confirmTransaction(rpc, transactionHash, {
    timeoutMs: CONFIRM_TIMEOUT_MS,
    pollIntervalMs: CONFIRM_POLL_INTERVAL_MS,
    onPoll: (attempt) => log("vault_deploy_poll", { transactionHash, attempt }),
  });

  const afterSubmit = await loadManifest(MANIFEST_PATH);

  if (outcome.status !== "success") {
    const failedRecord: DeploymentRecord = {
      ...submittedRecord,
      status: "failed",
      confirmedAtMs: Date.now(),
    };
    await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, failedRecord));
    log("vault_deploy_failed", {
      transactionHash,
      status: outcome.status,
      errorMessage: outcome.errorMessage,
      attempts: outcome.attempts,
    });
    throw new Error(
      outcome.status === "timeout"
        ? `Vault install ${transactionHash} not finalized within ${CONFIRM_TIMEOUT_MS}ms`
        : `Vault install ${transactionHash} reverted on-chain: ${outcome.errorMessage ?? "unknown error"}`,
    );
  }

  const confirmedRecord: DeploymentRecord = {
    ...submittedRecord,
    status: "confirmed",
    confirmedAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, confirmedRecord));

  log("vault_deploy_confirmed", {
    transactionHash,
    blockHash: outcome.blockHash,
    blockHeight: outcome.blockHeight,
    cost: outcome.cost,
    note: "Read the installed contract + package hash from the deploy's named keys on the explorer, then set VAULT_CONTRACT_HASH / VAULT_PACKAGE_HASH (and record them in .deployments.json so fund/agent can reuse them).",
  });

  return { transactionHash, skipped: false };
}

const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;
if (invokedDirectly) {
  deployVault().catch((err) => {
    log("fatal", { error: err instanceof Error ? err.message : String(err) });
    process.exitCode = 1;
  });
}
