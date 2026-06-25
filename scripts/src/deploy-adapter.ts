import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
const { CLValue, SessionBuilder } = casper;
import {
  loadSecp256k1,
  log,
  logNetworkBanner,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  odraInstallArgs,
  requireEnv,
} from "./lib/casper.js";
import { confirmTransaction } from "./lib/confirm.js";

const INSTALL_PAYMENT_MOTES = Number(process.env.ADAPTER_PAYMENT_MOTES ?? "250000000000");
const CONFIRM_TIMEOUT_MS = Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000");
const CONFIRM_POLL_INTERVAL_MS = Number(process.env.CONFIRM_POLL_INTERVAL_MS ?? "5000");

/**
 * Install the atomic on-chain Cep18SwapAdapter — the self-contained venue the vault
 * routes slices through (no external DEX needed). Owner = the install sender
 * (treasury). After confirmation the installed contract/package hashes live under
 * the sender's named keys; read them from the explorer / named keys and set
 * VENUE_ADDRESSES to the contract hash.
 */
async function main(): Promise<void> {
  logNetworkBanner("deploy-adapter");
  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const wasmPath =
    process.env.ADAPTER_WASM_PATH ?? "../contracts/dex-adapter/wasm/Cep18SwapAdapter.wasm";
  const venueId = process.env.VENUE_ALLOWLIST?.split(",")[0]?.trim() || "cspr.trade";
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const wasm = new Uint8Array(await readFile(wasmPath));
  const args = odraInstallArgs("cadence_adapter_package_hash", {
    venue_id: CLValue.newCLString(venueId),
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
  log("adapter_deploy_submitted", { transactionHash, chainName, venueId });

  const outcome = await confirmTransaction(rpc, transactionHash, {
    timeoutMs: CONFIRM_TIMEOUT_MS,
    pollIntervalMs: CONFIRM_POLL_INTERVAL_MS,
    onPoll: (attempt) => log("adapter_deploy_poll", { transactionHash, attempt }),
  });

  if (outcome.status !== "success") {
    log("adapter_deploy_failed", { transactionHash, status: outcome.status, error: outcome.errorMessage });
    throw new Error(
      outcome.status === "timeout"
        ? `Adapter install ${transactionHash} not finalized in ${CONFIRM_TIMEOUT_MS}ms`
        : `Adapter install ${transactionHash} reverted: ${outcome.errorMessage ?? "unknown"}`,
    );
  }

  log("adapter_deploy_confirmed", {
    transactionHash,
    blockHash: outcome.blockHash,
    explorer: `https://testnet.cspr.live/deploy/${transactionHash}`,
    note: "Read the installed Cep18SwapAdapter contract hash from the sender's named keys on the explorer, then set VENUE_ADDRESSES to it.",
  });
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
