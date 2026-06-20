import "dotenv/config";
import { access } from "node:fs/promises";
import { runAgent } from "@cadence/agent";
import { deployVault } from "./deploy.js";
import { fundVault } from "./fund.js";
import { log } from "./lib/casper.js";
import { assertDeployTargetAllowed } from "./lib/network-guard.js";

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

/**
 * End-to-end demo orchestrator. It performs as much of the flow as the current
 * environment allows, in order:
 *   1. Require a signed mandate (run `npm run sign-mandate` first).
 *   2. If VAULT_CONTRACT_HASH is unset, deploy the vault and stop with next steps
 *      (the installed contract hash must be read from the deploy before funding).
 *   3. Otherwise fund the vault and run the agent end to end on the testnet pair.
 */
async function main(): Promise<void> {
  // Deploy-safety: fail fast before any deploy/fund if mainnet isn't opted in.
  assertDeployTargetAllowed(process.env.CASPER_NETWORK ?? "testnet", process.env.ALLOW_MAINNET === "true");

  const signedPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";
  if (!(await exists(signedPath))) {
    throw new Error(`No signed mandate at ${signedPath}. Run: npm run sign-mandate -w @cadence/scripts`);
  }

  if (!process.env.VAULT_CONTRACT_HASH) {
    log("demo_step", { step: "deploy" });
    const { transactionHash } = await deployVault();
    log("demo_paused", {
      reason: "Vault deployed. Read the installed contract + package hash from this deploy, set VAULT_CONTRACT_HASH / VAULT_PACKAGE_HASH, then re-run the demo to fund and execute.",
      transactionHash,
    });
    return;
  }

  log("demo_step", { step: "fund" });
  await fundVault();

  log("demo_step", { step: "run-agent" });
  await runAgent();

  log("demo_complete");
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
