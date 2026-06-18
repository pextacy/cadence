import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
import type { SignedMandateFile } from "@cadence/agent";

const { Args, CLValue, Key, SessionBuilder } = casper;
import {
  clBytesList,
  clStringList,
  hexToBytes,
  loadSecp256k1,
  log,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  requireEnv,
} from "./lib/casper.js";

const INSTALL_PAYMENT_MOTES = Number(process.env.DEPLOY_PAYMENT_MOTES ?? "300000000000");

export interface DeployResult {
  transactionHash: string;
}

/** Deploy the Execution Vault WASM with the signed mandate's limits as init args. */
export async function deployVault(): Promise<DeployResult> {
  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const wasmPath = process.env.VAULT_WASM_PATH ?? "../contracts/wasm/ExecutionVault.wasm";
  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  const agentAccountHash = requireEnv("AGENT_ACCOUNT_HASH"); // "account-hash-…"
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const wasm = new Uint8Array(await readFile(wasmPath));
  const signed = JSON.parse(await readFile(signedPath, "utf8")) as SignedMandateFile;
  const m = signed.mandate;

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

  log("vault_deploy_submitted", {
    transactionHash,
    chainName,
    note: "Read the installed contract + package hash from the deploy's named keys on the explorer, then set VAULT_CONTRACT_HASH / VAULT_PACKAGE_HASH.",
  });
  return { transactionHash };
}

const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;
if (invokedDirectly) {
  deployVault().catch((err) => {
    log("fatal", { error: err instanceof Error ? err.message : String(err) });
    process.exitCode = 1;
  });
}
