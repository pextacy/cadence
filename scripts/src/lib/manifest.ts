import { readFile, rename, writeFile } from "node:fs/promises";
import { z } from "zod";

/**
 * One immutable entry in `.deployments.json`. The idempotency key is
 * `(kind, chainName, mandateDigest)`: a matching `confirmed` record
 * short-circuits a re-run. `contractHash` is filled in after a vault-install
 * confirms so fund/agent can reuse it without manual env editing.
 */
export interface DeploymentRecord {
  readonly kind: "vault-install" | "vault-fund" | "vault-register";
  readonly chainName: string;
  readonly mandateDigest: string;
  readonly transactionHash: string;
  readonly contractHash?: string;
  readonly packageHash?: string;
  readonly amount?: string;
  readonly status: "submitted" | "confirmed" | "failed";
  readonly createdAtMs: number;
  readonly confirmedAtMs?: number;
}

/**
 * Top-level shape of `.deployments.json` (git-ignored, chain-scoped). Always
 * read/written as a whole; updates produce a new manifest object (immutable).
 */
export interface DeploymentManifest {
  readonly version: 1;
  readonly deployments: readonly DeploymentRecord[];
}

/** The idempotency key uniquely identifying a deployment action. */
export interface DeploymentKey {
  readonly kind: DeploymentRecord["kind"];
  readonly chainName: string;
  readonly mandateDigest: string;
}

const recordSchema = z.object({
  kind: z.enum(["vault-install", "vault-fund", "vault-register"]),
  chainName: z.string().min(1),
  mandateDigest: z.string().min(1),
  transactionHash: z.string().min(1),
  contractHash: z.string().optional(),
  packageHash: z.string().optional(),
  amount: z.string().optional(),
  status: z.enum(["submitted", "confirmed", "failed"]),
  createdAtMs: z.number(),
  confirmedAtMs: z.number().optional(),
});

const manifestSchema = z.object({
  version: z.literal(1),
  deployments: z.array(recordSchema),
});

const EMPTY_MANIFEST: DeploymentManifest = { version: 1, deployments: [] };

/**
 * Reads `.deployments.json`, validating against the zod schema; returns
 * `{ version: 1, deployments: [] }` when the file is absent. Throws on
 * malformed / incompatible-version content (fail fast at the boundary).
 */
export async function loadManifest(path: string): Promise<DeploymentManifest> {
  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch (error: unknown) {
    if (isNotFound(error)) return EMPTY_MANIFEST;
    throw new Error(`Failed to read deployment manifest at ${path}: ${messageOf(error)}`);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error: unknown) {
    throw new Error(`Deployment manifest at ${path} is not valid JSON: ${messageOf(error)}`);
  }

  const result = manifestSchema.safeParse(parsed);
  if (!result.success) {
    throw new Error(`Deployment manifest at ${path} failed validation: ${result.error.message}`);
  }
  return result.data;
}

/**
 * Pure lookup of the idempotency key. `deploy.ts` / `fund.ts` call this first;
 * if a `confirmed` record exists they log a skip and reuse its hashes instead
 * of submitting.
 */
export function findRecord(
  manifest: DeploymentManifest,
  key: DeploymentKey,
): DeploymentRecord | undefined {
  return manifest.deployments.find(
    (r) =>
      r.kind === key.kind &&
      r.chainName === key.chainName &&
      r.mandateDigest === key.mandateDigest,
  );
}

/**
 * Returns a NEW manifest with `record` added or replaced by idempotency key —
 * never mutates the input array (spread + filter).
 */
export function upsertRecord(
  manifest: DeploymentManifest,
  record: DeploymentRecord,
): DeploymentManifest {
  const others = manifest.deployments.filter(
    (r) =>
      !(
        r.kind === record.kind &&
        r.chainName === record.chainName &&
        r.mandateDigest === record.mandateDigest
      ),
  );
  return { version: 1, deployments: [...others, record] };
}

/**
 * Atomically persists the manifest (write tmp + rename) as pretty JSON. Called
 * after submit (status `submitted`) and again after `confirmTransaction`
 * resolves (status `confirmed` | `failed`).
 */
export async function saveManifest(path: string, manifest: DeploymentManifest): Promise<void> {
  const validated = manifestSchema.parse(manifest);
  const tmpPath = `${path}.${process.pid}.${Date.now()}.tmp`;
  const json = `${JSON.stringify(validated, null, 2)}\n`;
  await writeFile(tmpPath, json, "utf8");
  await rename(tmpPath, path);
}

function isNotFound(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  );
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
