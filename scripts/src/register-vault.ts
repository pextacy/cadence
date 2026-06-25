import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
import type { SignedMandateFile } from "@cadence/agent";

const { Args, CLValue, Key, ContractCallBuilder } = casper;
import {
  clByteArray32,
  hexToBytes,
  loadSecp256k1,
  log,
  logNetworkBanner,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  requireEnv,
} from "./lib/casper.js";
import { confirmTransaction } from "./lib/confirm.js";
import {
  findRecord,
  loadManifest,
  saveManifest,
  upsertRecord,
  type DeploymentRecord,
} from "./lib/manifest.js";

const REGISTER_PAYMENT_MOTES = Number(process.env.REGISTER_PAYMENT_MOTES ?? "3000000000");
const MANIFEST_PATH = process.env.DEPLOYMENTS_MANIFEST_PATH ?? "./.deployments.json";
const CONFIRM_TIMEOUT_MS = Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000");
const CONFIRM_POLL_INTERVAL_MS = Number(process.env.CONFIRM_POLL_INTERVAL_MS ?? "5000");

export interface RegisterResult {
  transactionHash: string;
  /** True when an existing confirmed registration was reused instead of re-submitted. */
  skipped: boolean;
}

/**
 * Register a deployed Execution Vault in the desk-wide `VaultRegistry` so the
 * Guardian can enumerate and pause it. Mirrors `fund.ts`'s
 * `ContractCallBuilder.byHash(...).entryPoint(...)` pattern: a treasury-signed call
 * to the registry's `register(vault, treasury, mandate_hash)` entrypoint.
 *
 * Idempotent + finality-confirmed per `(chain, mandateDigest)`: a prior `confirmed`
 * registration short-circuits; otherwise the submission is recorded, polled to
 * finality, then recorded `confirmed` / `failed`.
 *
 * Inputs (env):
 *  - `REGISTRY_CONTRACT_HASH` — the registry contract hash this call targets.
 *  - `VAULT_REGISTER_ADDRESS` (falls back to `VAULT_CONTRACT_HASH`) — the vault's
 *    on-chain Casper Key to register. This is the value the registry stores and the
 *    Guardian later calls `pause()` on, so it MUST be the vault's addressable
 *    contract Key (the same hash the agent calls), not the package hash if they
 *    differ on your node — verify against the live deploy's named keys.
 *  - `TREASURY_ACCOUNT_HASH` — the treasury Address recorded alongside the vault.
 *  - the signed mandate (`SIGNED_MANDATE_PATH`) — supplies the 32-byte mandate hash.
 *  - `TREASURY_PRIVATE_KEY` — signs the call; the treasury holds the registry's
 *    writer role by default (the registry bootstraps its deployer as writer).
 */
export async function registerVault(): Promise<RegisterResult> {
  logNetworkBanner("register-vault");
  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const registryHash = requireEnv("REGISTRY_CONTRACT_HASH");
  const vaultAddress = process.env.VAULT_REGISTER_ADDRESS ?? requireEnv("VAULT_CONTRACT_HASH");
  const treasuryAccountHash = requireEnv("TREASURY_ACCOUNT_HASH"); // "account-hash-…"
  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const signed = JSON.parse(await readFile(signedPath, "utf8")) as SignedMandateFile;
  const mandateDigest = signed.digest;
  const mandateHashBytes = hexToBytes(mandateDigest);
  if (mandateHashBytes.length !== 32) {
    throw new Error(
      `Mandate digest must be 32 bytes for the registry mandate_hash, got ${mandateHashBytes.length}.`,
    );
  }

  const manifest = await loadManifest(MANIFEST_PATH);

  // Idempotency: reuse a prior confirmed registration for the same chain + mandate.
  const existing = findRecord(manifest, { kind: "vault-register", chainName, mandateDigest });
  if (existing?.status === "confirmed") {
    log("vault_register_skipped", {
      reason: "already_confirmed",
      chainName,
      mandateDigest,
      transactionHash: existing.transactionHash,
      vaultAddress,
      registryHash,
    });
    return { transactionHash: existing.transactionHash, skipped: true };
  }

  const tx = new ContractCallBuilder()
    .from(treasuryKey.publicKey)
    .byHash(registryHash)
    .entryPoint("register")
    .runtimeArgs(
      Args.fromMap({
        vault: CLValue.newCLKey(Key.newKey(vaultAddress)),
        treasury: CLValue.newCLKey(Key.newKey(treasuryAccountHash)),
        mandate_hash: clByteArray32(mandateHashBytes),
      }),
    )
    .chainName(chainName)
    .payment(REGISTER_PAYMENT_MOTES)
    .build();
  tx.sign(treasuryKey);

  const rpc = makeRpc(nodeRpc);
  const result = await rpc.putTransaction(tx);
  const transactionHash = result.transactionHash.toJSON();

  log("vault_register_submitted", { transactionHash, vaultAddress, registryHash, mandateDigest });

  const submittedRecord: DeploymentRecord = {
    kind: "vault-register",
    chainName,
    mandateDigest,
    transactionHash,
    contractHash: vaultAddress,
    status: "submitted",
    createdAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(manifest, submittedRecord));

  const outcome = await confirmTransaction(rpc, transactionHash, {
    timeoutMs: CONFIRM_TIMEOUT_MS,
    pollIntervalMs: CONFIRM_POLL_INTERVAL_MS,
    onPoll: (attempt) => log("vault_register_poll", { transactionHash, attempt }),
  });

  const afterSubmit = await loadManifest(MANIFEST_PATH);

  if (outcome.status !== "success") {
    const failedRecord: DeploymentRecord = {
      ...submittedRecord,
      status: "failed",
      confirmedAtMs: Date.now(),
    };
    await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, failedRecord));
    log("vault_register_failed", {
      transactionHash,
      status: outcome.status,
      errorMessage: outcome.errorMessage,
      attempts: outcome.attempts,
    });
    throw new Error(
      outcome.status === "timeout"
        ? `Vault registration ${transactionHash} not finalized within ${CONFIRM_TIMEOUT_MS}ms`
        : `Vault registration ${transactionHash} reverted on-chain: ${outcome.errorMessage ?? "unknown error"}`,
    );
  }

  const confirmedRecord: DeploymentRecord = {
    ...submittedRecord,
    status: "confirmed",
    confirmedAtMs: Date.now(),
  };
  await saveManifest(MANIFEST_PATH, upsertRecord(afterSubmit, confirmedRecord));

  log("vault_register_confirmed", {
    transactionHash,
    vaultAddress,
    registryHash,
    blockHash: outcome.blockHash,
    blockHeight: outcome.blockHeight,
    cost: outcome.cost,
  });

  return { transactionHash, skipped: false };
}

const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;
if (invokedDirectly) {
  registerVault().catch((err) => {
    log("fatal", { error: err instanceof Error ? err.message : String(err) });
    process.exitCode = 1;
  });
}
