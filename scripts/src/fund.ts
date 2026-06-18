import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
import type { SignedMandateFile } from "@cadence/agent";

const { Args, CLValue, ContractCallBuilder } = casper;
import {
  loadSecp256k1,
  log,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  requireEnv,
} from "./lib/casper.js";

const FUND_PAYMENT_MOTES = Number(process.env.FUND_PAYMENT_MOTES ?? "5000000000");

export interface FundResult {
  transactionHash: string;
}

/**
 * Fund the vault with the mandate's full sell amount. The vault's `fund`
 * entrypoint is `#[odra(payable)]`; the attached CSPR is conveyed via Odra's
 * payable calling convention as the `amount` runtime argument.
 */
export async function fundVault(): Promise<FundResult> {
  const nodeRpc = networkNodeRpc();
  const chainName = networkChainName();
  const contractHash = requireEnv("VAULT_CONTRACT_HASH");
  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  const treasuryKey = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));

  const signed = JSON.parse(await readFile(signedPath, "utf8")) as SignedMandateFile;
  const total = signed.mandate.totalSellAmount;

  const tx = new ContractCallBuilder()
    .from(treasuryKey.publicKey)
    .byHash(contractHash)
    .entryPoint("fund")
    .runtimeArgs(Args.fromMap({ amount: CLValue.newCLUInt512(total) }))
    .chainName(chainName)
    .payment(FUND_PAYMENT_MOTES)
    .build();
  tx.sign(treasuryKey);

  const rpc = makeRpc(nodeRpc);
  const result = await rpc.putTransaction(tx);
  const transactionHash = result.transactionHash.toJSON();
  log("vault_funded_submitted", { transactionHash, amount: total });
  return { transactionHash };
}

const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;
if (invokedDirectly) {
  fundVault().catch((err) => {
    log("fatal", { error: err instanceof Error ? err.message : String(err) });
    process.exitCode = 1;
  });
}
