import "dotenv/config";
import { writeFile } from "node:fs/promises";
import {
  buildMandateDomain,
  humanSummary,
  signMandate,
  toBaseUnits,
  type Mandate,
} from "@cadence/mandate";
import {
  loadSecp256k1,
  log,
  logNetworkBanner,
  networkChainName,
  requireEnv,
  signCasperMandate,
} from "./lib/casper.js";

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
  logNetworkBanner("sign-mandate");
  const sellAsset = process.env.SELL_ASSET ?? "CSPR";
  const buyAsset = requireEnv("BUY_ASSET");
  const totalHuman = requireEnv("MANDATE_TOTAL_SIZE");
  const windowHours = Number(process.env.MANDATE_WINDOW_HOURS ?? "72");
  const slippagePct = Number(process.env.MANDATE_SLIPPAGE_PCT ?? "1.0");
  const venues = (process.env.VENUE_ALLOWLIST ?? "cspr.trade")
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean);
  const venueAddresses = (process.env.VENUE_ADDRESSES ?? "")
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean);
  if (venueAddresses.length !== venues.length) {
    throw new Error(
      `VENUE_ADDRESSES must list one Casper address per venue in VENUE_ALLOWLIST ` +
        `(${venues.length} venue(s), got ${venueAddresses.length} address(es)). ` +
        `The vault releases each slice only to these mandate-bound addresses.`,
    );
  }
  const chainName = networkChainName();
  const treasuryKey = requireEnv("TREASURY_PRIVATE_KEY");
  // The agent identity is bound into the Casper-native preimage the vault verifies
  // on-chain, so it must be known at signing time (offline, no chain access).
  const agentAccountHash = requireEnv("AGENT_ACCOUNT_HASH");
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
    venueAddresses,
    nonce: randomNonce(),
  };

  const domain = buildMandateDomain(chainName);
  const signed = signMandate(mandate, domain, treasuryKey);

  // The EIP-712 digest above is the human-readable artifact; the vault's `init`
  // actually authorizes the limits by verifying a Casper-native signature over the
  // frozen preimage. Produce it with the same treasury key (the install sender).
  const m = signed.mandate;
  const auth = signCasperMandate(loadSecp256k1(treasuryKey), {
    agentAccountHash,
    sellAsset: m.sellAsset,
    buyAsset: m.buyAsset,
    totalSell: BigInt(m.totalSellAmount),
    endTimeMs: BigInt(m.endTime) * 1000n,
    maxSlippageBps: m.maxSlippageBps,
    priceFloor: BigInt(m.priceFloor),
    priceCeiling: BigInt(m.priceCeiling),
    venues: m.venueAllowlist,
    venueAddresses: m.venueAddresses,
    nonceHex: m.nonce,
  });

  const out = {
    ...signed,
    agentAccountHash,
    casperSignature: auth.casperSignature,
    treasuryPublicKey: auth.treasuryPublicKey,
  };
  await writeFile(outPath, JSON.stringify(out, null, 2), "utf8");

  log("mandate_signed", { outPath, signer: signed.signer, digest: signed.digest });
  log("mandate_casper_auth", {
    treasuryPublicKey: auth.treasuryPublicKey,
    agentAccountHash,
  });
  log("mandate_summary", { summary: humanSummary(signed.mandate) });
}

main().catch((err) => {
  log("fatal", { error: err instanceof Error ? err.message : String(err) });
  process.exitCode = 1;
});
