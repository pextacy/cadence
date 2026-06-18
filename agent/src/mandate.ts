import { readFile } from "node:fs/promises";
import type { Mandate } from "@cadence/mandate";
import type { RuntimeMandate } from "./types.js";

/** A signed mandate as persisted by the signing step. */
export interface SignedMandateFile {
  mandate: Mandate;
  digest: string;
  signature: string;
  signer: string;
  /** Vault package hash the mandate was bound to. */
  vaultPackageHash?: string;
}

/** Load a signed mandate JSON file from disk. */
export async function loadSignedMandate(path: string): Promise<SignedMandateFile> {
  const text = await readFile(path, "utf8");
  return JSON.parse(text) as SignedMandateFile;
}

/**
 * Convert a signed mandate into the executor's runtime representation. Times are
 * converted from unix seconds (mandate) to milliseconds (vault / block time).
 */
export function toRuntimeMandate(m: Mandate): RuntimeMandate {
  return {
    totalSell: BigInt(m.totalSellAmount),
    endTimeMs: m.endTime * 1000,
    maxSlippageBps: m.maxSlippageBps,
    priceFloor: BigInt(m.priceFloor),
    priceCeiling: BigInt(m.priceCeiling),
    venueAllowlist: m.venueAllowlist,
    venueAddresses: m.venueAddresses,
    strategy: m.strategy,
  };
}
