import "dotenv/config";
import casper from "casper-js-sdk";
const { Args, CLValue, ContractCallBuilder } = casper;
import {
  loadSecp256k1,
  log,
  logNetworkBanner,
  makeRpc,
  networkChainName,
  networkNodeRpc,
  requireEnv,
} from "./lib/casper.js";
import { confirmTransaction } from "./lib/confirm.js";

const GAS = Number(process.env.SLICE_PAYMENT_MOTES ?? "15000000000");
const CONFIRM_TIMEOUT_MS = Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000");

/**
 * Drive a single `execute_slice` against the live vault with the AGENT key — the
 * constrained, agent-only spend entrypoint that re-validates every guardrail
 * on-chain. Demonstrates the core safety property: the agent can only release a
 * slice within the signed mandate's limits, enforced by the contract.
 */
async function main(): Promise<void> {
  logNetworkBanner("execute-slice");
  const chainName = networkChainName();
  const rpc = makeRpc(networkNodeRpc());
  const agentKey = loadSecp256k1(requireEnv("AGENT_PRIVATE_KEY"));
  const vaultPkg = requireEnv("VAULT_CONTRACT_HASH").replace(/^(hash-|contract-package-)/, "");
  const venue = (process.env.VENUE_ALLOWLIST ?? "cspr.trade").split(",")[0]!.trim();

  // A 10 CSPR slice quoted at price 2.0 (20 buy units), min_out at the mandate's
  // 1% cap — passes every guardrail (cap, deadline, slippage, venue).
  const sellAmount = 10_000_000_000n; // 10 CSPR
  const quotedOut = 20_000_000_000n;
  // min_out at the 1% cap, rounded up to satisfy the vault's cross-multiply check.
  const minOut = (quotedOut * 9900n + 9999n) / 10_000n;

  const args = Args.fromMap({
    sell_amount: CLValue.newCLUInt512(sellAmount.toString()),
    quoted_out: CLValue.newCLUInt512(quotedOut.toString()),
    min_out: CLValue.newCLUInt512(minOut.toString()),
    venue: CLValue.newCLString(venue),
  });

  const tx = new ContractCallBuilder()
    .from(agentKey.publicKey)
    .byPackageHash(vaultPkg)
    .entryPoint("execute_slice")
    .runtimeArgs(args)
    .chainName(chainName)
    .payment(GAS)
    .build();
  tx.sign(agentKey);

  const result = await rpc.putTransaction(tx);
  const transactionHash = result.transactionHash.toJSON();
  log("slice_submitted", { transactionHash, sellAmount: sellAmount.toString(), minOut: minOut.toString(), venue });

  const outcome = await confirmTransaction(rpc, transactionHash, {
    timeoutMs: CONFIRM_TIMEOUT_MS,
    onPoll: (attempt) => log("slice_poll", { transactionHash, attempt }),
  });
  if (outcome.status !== "success") {
    throw new Error(`execute_slice ${transactionHash} ${outcome.status}: ${outcome.errorMessage ?? ""}`);
  }
  log("slice_confirmed", {
    transactionHash,
    blockHeight: outcome.blockHeight,
    cost: outcome.cost,
    explorer: `https://testnet.cspr.live/deploy/${transactionHash}`,
  });
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
