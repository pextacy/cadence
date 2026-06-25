import "dotenv/config";
import { readFile } from "node:fs/promises";
import casper from "casper-js-sdk";
const { Args, CLValue, ContractCallBuilder, SessionBuilder } = casper;
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

const strip = (h: string) => h.replace(/^(hash-|contract-package-)/, "");
const CONFIRM = { timeoutMs: Number(process.env.CONFIRM_TIMEOUT_MS ?? "180000") };

/**
 * Complete the on-chain atomic-swap path end to end:
 *   1. set_venue_adapter      — treasury flips the venue to the adapter route
 *      (the legacy native transfer cannot target a contract — TransferToContract).
 *   2. adapter.set_price(2.0) — owner sets the pool price (1e6 scale).
 *   3. adapter.seed_reserve   — treasury funds the adapter's payout reserve (payable).
 *   4. execute_slice          — AGENT releases a 10 CSPR slice; the vault calls the
 *      adapter atomically, which pays the treasury and the fill is recorded.
 */
async function main(): Promise<void> {
  logNetworkBanner("enable-and-slice");
  const chainName = networkChainName();
  const rpc = makeRpc(networkNodeRpc());
  const treasury = loadSecp256k1(requireEnv("TREASURY_PRIVATE_KEY"));
  const agent = loadSecp256k1(requireEnv("AGENT_PRIVATE_KEY"));
  const vault = strip(requireEnv("VAULT_CONTRACT_HASH"));
  const adapterPkg = requireEnv("VENUE_ADDRESSES").split(",")[0]!.trim();
  const adapter = strip(adapterPkg);
  const venue = (process.env.VENUE_ALLOWLIST ?? "cspr.trade").split(",")[0]!.trim();
  const proxyWasm = new Uint8Array(await readFile("./resources/proxy_caller_with_return.wasm"));

  const send = async (label: string, key: typeof treasury, build: () => casper.Transaction) => {
    const tx = build();
    tx.sign(key);
    const res = await rpc.putTransaction(tx);
    const h = res.transactionHash.toJSON();
    log(`${label}_submitted`, { tx: h });
    const out = await confirmTransaction(rpc, h, CONFIRM);
    if (out.status !== "success") throw new Error(`${label} ${out.status}: ${out.errorMessage ?? ""}`);
    log(`${label}_confirmed`, { tx: h, block: out.blockHeight, explorer: `https://testnet.cspr.live/deploy/${h}` });
  };

  // 1. set_venue_adapter(venue, true) — treasury
  await send("set_venue_adapter", treasury, () =>
    new ContractCallBuilder().from(treasury.publicKey).byPackageHash(vault)
      .entryPoint("set_venue_adapter")
      .runtimeArgs(Args.fromMap({ venue: CLValue.newCLString(venue), is_adapter: CLValue.newCLValueBool(true) }))
      .chainName(chainName).payment(5_000_000_000).build(),
  );

  // 2. adapter.set_price(2.0) — owner (treasury); 1e6 scale → 2_000_000
  await send("set_price", treasury, () =>
    new ContractCallBuilder().from(treasury.publicKey).byPackageHash(adapter)
      .entryPoint("set_price")
      .runtimeArgs(Args.fromMap({ price: CLValue.newCLUInt512("2000000") }))
      .chainName(chainName).payment(5_000_000_000).build(),
  );

  // 3. adapter.seed_reserve — payable, fund 40 CSPR of payout reserve (proxy)
  await send("seed_reserve", treasury, () =>
    new SessionBuilder().from(treasury.publicKey).wasm(proxyWasm)
      .runtimeArgs(proxyCallArgs(adapterPkg, "seed_reserve", EMPTY_RUNTIME_ARGS, 40_000_000_000n))
      .chainName(chainName).payment(15_000_000_000).build(),
  );

  // 4. execute_slice — AGENT; 10 CSPR @ price 2.0, min_out at 1% cap
  const sellAmount = 10_000_000_000n;
  const quotedOut = 20_000_000_000n;
  const minOut = (quotedOut * 9900n + 9999n) / 10_000n;
  await send("execute_slice", agent, () =>
    new ContractCallBuilder().from(agent.publicKey).byPackageHash(vault)
      .entryPoint("execute_slice")
      .runtimeArgs(Args.fromMap({
        sell_amount: CLValue.newCLUInt512(sellAmount.toString()),
        quoted_out: CLValue.newCLUInt512(quotedOut.toString()),
        min_out: CLValue.newCLUInt512(minOut.toString()),
        venue: CLValue.newCLString(venue),
      }))
      .chainName(chainName).payment(20_000_000_000).build(),
  );

  log("done", { note: "atomic slice executed: vault → adapter swap → treasury paid, fill recorded" });
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
