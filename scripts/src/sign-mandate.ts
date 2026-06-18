import "dotenv/config";
import { writeFile } from "node:fs/promises";
import {
  buildMandateDomain,
  humanSummary,
  signMandate,
  toBaseUnits,
  type Mandate,
} from "@cadence/mandate";
import { log, networkChainName, requireEnv } from "./lib/casper.js";

function toFixedPrice(human: string | undefined): string {
  if (!human || !human.trim()) return "0";
  const [whole, frac = ""] = human.trim().split(".");
  const fracPadded = (frac + "0".repeat(9)).slice(0, 9);
  return BigInt((whole || "0") + fracPadded).toString();
}

function randomNonce(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return "0x" + Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

/**
 * Build a mandate from environment configuration and sign it with the treasury
 * key under the chain-scoped EIP-712 domain (not bound to any vault package — the
 * vault commits to this signed digest on `init`). Writes the signed mandate to
 * `SIGNED_MANDATE_PATH`. Runs fully offline — no chain access required.
 */
async function main(): Promise<void> {
  const sellAsset = process.env.SELL_ASSET ?? "CSPR";
  const buyAsset = requireEnv("BUY_ASSET");
  const totalHuman = requireEnv("MANDATE_TOTAL_SIZE");
  const windowHours = Number(process.env.MANDATE_WINDOW_HOURS ?? "72");
  const slippagePct = Number(process.env.MANDATE_SLIPPAGE_PCT ?? "1.0");
  const venues = (process.env.VENUE_ALLOWLIST ?? "cspr.trade").split(",").map((v) => v.trim());
  const chainName = networkChainName();
  const treasuryKey = requireEnv("TREASURY_PRIVATE_KEY");
  const outPath = process.env.SIGNED_MANDATE_PATH ?? "./mandate.signed.json";

  const startTime = Math.floor(Date.now() / 1000);
  const endTime = startTime + Math.round(windowHours * 3600);

  const mandate: Omit<Mandate, "treasury"> = {
    version: 1,
    sellAsset,
    buyAsset,
    totalSellAmount: toBaseUnits(totalHuman, sellAsset).toString(),
    startTime,
    endTime,
    maxSlippageBps: Math.round(slippagePct * 100),
    priceFloor: toFixedPrice(process.env.MANDATE_PRICE_FLOOR),
    priceCeiling: toFixedPrice(process.env.MANDATE_PRICE_CEILING),
    strategy: (process.env.MANDATE_STRATEGY as Mandate["strategy"]) ?? "TWAP",
    venueAllowlist: venues,
    nonce: randomNonce(),
  };

  const domain = buildMandateDomain(chainName);
  const signed = signMandate(mandate, domain, treasuryKey);

  await writeFile(outPath, JSON.stringify(signed, null, 2), "utf8");

  log("mandate_signed", { outPath, signer: signed.signer, digest: signed.digest });
  log("mandate_summary", { summary: humanSummary(signed.mandate) });
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
