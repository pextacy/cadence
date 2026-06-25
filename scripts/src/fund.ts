import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
import type { SignedMandateFile } from "@cadence/agent";

const { SessionBuilder } = casper;
import {
  EMPTY_RUNTIME_ARGS,
  loadSecp256k1,
  log,
  logNetworkBanner,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  proxyCallArgs,
  requireEnv,
} from "./lib/casper.js";
import { confirmTransaction } from "./lib/confirm.js";
import {
  findRecord,
  loadManifest,
  saveManifest,
  upsertRecord,
  type DeploymentManifest,
  type DeploymentRecord,
} from "./lib/manifest.js";

const FUND_PAYMENT_MOTES = Number(process.env.FUND_PAYMENT_MOTES ?? "15000000000");
const PROXY_WASM_PATH =
  process.env.PROXY_WASM_PATH ?? "./resources/proxy_caller_with_return.wasm";
const MANIFEST_PATH = process.env.DEPLOYMENTS_MANIFEST_PATH ?? "./.deployments.json";
const CONFIRM_TIMEOUT_MS = Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000");
const CONFIRM_POLL_INTERVAL_MS = Number(process.env.CONFIRM_POLL_INTERVAL_MS ?? "5000");

export interface FundResult {
  transactionHash: string;
  /** True when an existing confirmed fund was reused instead of re-submitted. */
  skipped: boolean;
}

/**
 * Fund the vault with the mandate's full sell amount. The vault's `fund`
 * entrypoint is `#[odra(payable)]`; the attached CSPR is conveyed through Odra's
 * proxy_caller session (see {@link proxyCallArgs}), which moves the value from the
 * treasury's main purse into a cargo purse the contract receives.
 *
 * Idempotent + finality-confirmed: a prior `confirmed` fund for this
 * `(chain, mandateDigest)` short-circuits; otherwise the submission is recorded,
 * polled to finality, then recorded `confirmed` / `failed`. The vault contract
 * hash is read from `VAULT_CONTRACT_HASH` or, when unset, from the manifest's
 * confirmed install record.
 */
export async function fundVault(): Promise<FundResult> {
  logNetworkBanner("fund-vault");
  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const signed = JSON.parse(await readFile(signedPath, "utf8")) as SignedMandateFile;
  const total = signed.mandate.totalSellAmount;
  const mandateDigest = signed.digest;

  const manifest = await loadManifest(MANIFEST_PATH);

  // Idempotency: reuse a prior confirmed fund for the same chain + mandate.
  const existing = findRecord(manifest, { kind: "vault-fund", chainName, mandateDigest });
  if (existing?.status === "confirmed") {
    log("vault_fund_skipped", {
      reason: "already_confirmed",
      chainName,
      mandateDigest,
      transactionHash: existing.transactionHash,
      amount: existing.amount,
    });
    return { transactionHash: existing.transactionHash, skipped: true };
  }

  const contractHash = resolveContractHash(manifest, chainName, mandateDigest);

  // `fund` is `#[odra(payable)]`: it reads attached_value(), so the CSPR must be
  // attached. A plain stored-contract call cannot carry value — route through
  // Odra's proxy_caller session, which moves `total` from the treasury's main purse
  // into a cargo purse and invokes `fund` with it. (payment below is gas only.)
  const proxyWasm = new Uint8Array(await readFile(PROXY_WASM_PATH));
  const args = proxyCallArgs(contractHash, "fund", EMPTY_RUNTIME_ARGS, BigInt(total));
  const tx = new SessionBuilder()
    .from(treasuryKey.publicKey)
    .wasm(proxyWasm)
    .runtimeArgs(args)
    .chainName(chainName)
    .payment(FUND_PAYMENT_MOTES)
    .build();
  tx.sign(treasuryKey);

  const rpc = makeRpc(nodeRpc);
  const result = await rpc.putTransaction(tx);
  const transactionHash = result.transactionHash.toJSON();

  log("vault_fund_submitted", { transactionHash, amount: total, contractHash });

  const submittedRecord: DeploymentRecord = {
    kind: "vault-fund",
    chainName,
    mandateDigest,
    transactionHash,
    contractHash,
    amount: total,
    status: "submitted",
    createdAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(manifest, submittedRecord));

  const outcome = await confirmTransaction(rpc, transactionHash, {
    timeoutMs: CONFIRM_TIMEOUT_MS,
    pollIntervalMs: CONFIRM_POLL_INTERVAL_MS,
    onPoll: (attempt) => log("vault_fund_poll", { transactionHash, attempt }),
  });

  const afterSubmit = await loadManifest(MANIFEST_PATH);

  if (outcome.status !== "success") {
    const failedRecord: DeploymentRecord = {
      ...submittedRecord,
      status: "failed",
      confirmedAtMs: Date.now(),
    };
    await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, failedRecord));
    log("vault_fund_failed", {
      transactionHash,
      status: outcome.status,
      errorMessage: outcome.errorMessage,
      attempts: outcome.attempts,
    });
    throw new Error(
      outcome.status === "timeout"
        ? `Vault fund ${transactionHash} not finalized within ${CONFIRM_TIMEOUT_MS}ms`
        : `Vault fund ${transactionHash} reverted on-chain: ${outcome.errorMessage ?? "unknown error"}`,
    );
  }

  const confirmedRecord: DeploymentRecord = {
    ...submittedRecord,
    status: "confirmed",
    confirmedAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, confirmedRecord));

  log("vault_fund_confirmed", {
    transactionHash,
    amount: total,
    blockHash: outcome.blockHash,
    blockHeight: outcome.blockHeight,
    cost: outcome.cost,
  });

  return { transactionHash, skipped: false };
}

/**
 * Resolve the vault contract hash from the env override, falling back to the
 * confirmed install record in the manifest (so fund does not require manual env
 * editing after a `deployVault` that recorded the hash).
 */
function resolveContractHash(
  manifest: DeploymentManifest,
  chainName: string,
  mandateDigest: string,
): string {
  const fromEnv = process.env.VAULT_CONTRACT_HASH;
  if (fromEnv !== undefined && fromEnv !== "") return fromEnv;

  const install = findRecord(manifest, { kind: "vault-install", chainName, mandateDigest });
  if (install?.status === "confirmed" && install.contractHash) {
    return install.contractHash;
  }

  throw new Error(
    "Missing vault contract hash: set VAULT_CONTRACT_HASH, or record contractHash on " +
      "the confirmed vault-install entry in .deployments.json before funding.",
  );
}

const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;
if (invokedDirectly) {
  fundVault().catch((err) => {
    log("fatal", { error: err instanceof Error ? err.message : String(err) });
    process.exitCode = 1;
  });
}
