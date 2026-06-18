import { readFile } from "node:fs/promises";
import { z } from "zod";

/** One entry: a signed mandate file paired with the vault it is bound to. */
const PortfolioEntrySchema = z.object({
  signedMandatePath: z.string().min(1),
  vaultContractHash: z.string().min(1),
  /** Optional per-mandate proceeds recipient; falls back to TREASURY_ACCOUNT_HASH. */
  treasuryAccountHash: z.string().min(1).optional(),
});

/**
 * A portfolio manifest lists the mandates the agent manages concurrently. Vault
 * contract hashes must be unique — one vault per mandate keeps custody isolated,
 * so the same vault appearing twice is a configuration error, not a portfolio.
 */
const PortfolioManifestSchema = z
  .object({ mandates: z.array(PortfolioEntrySchema).min(1) })
  .superRefine((m, ctx) => {
    const seen = new Set<string>();
    for (const entry of m.mandates) {
      if (seen.has(entry.vaultContractHash)) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
          message: `duplicate vault contract hash: ${entry.vaultContractHash}`,
        });
      }
      seen.add(entry.vaultContractHash);
    }
  });

export type PortfolioEntry = z.infer<typeof PortfolioEntrySchema>;
export type PortfolioManifest = z.infer<typeof PortfolioManifestSchema>;

/** Validate raw manifest data. Throws (Zod) on any structural problem. */
export function parsePortfolioManifest(raw: unknown): PortfolioManifest {
  return PortfolioManifestSchema.parse(raw);
}

/** Load and validate a portfolio manifest JSON file from disk. */
export async function loadPortfolioManifest(path: string): Promise<PortfolioManifest> {
  const text = await readFile(path, "utf8");
  return parsePortfolioManifest(JSON.parse(text));
}
