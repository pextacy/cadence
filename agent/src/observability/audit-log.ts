/**
 * AuditLog: a tamper-evident, append-only, hash-chained record of every
 * decision the agent makes — each proposal, signature, tx hash and x402 payment
 * proof — enforcing the no-silent-trades invariant the executor already aims
 * for. `runtime.log()` emits the human-readable mirror so the structured log
 * line and the audit chain never diverge.
 *
 * The chain is a sha256 link over the previous entry's hash plus the canonical
 * JSON of the current entry, exactly like a minimal blockchain: any retroactive
 * edit breaks `verify()`. Persistence is an append-only JSONL file written with
 * `fs.appendFile` (ordered, durable on flush). Uses only node builtins.
 */
import { createHash } from "node:crypto";
import { appendFile, readFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";

export interface AuditEntry {
  readonly event: string;
  readonly trackId?: string;
  readonly sliceId?: number;
  readonly detail: Readonly<Record<string, unknown>>;
  /** Hash of the previous chained entry; set by the log, not the caller. */
  readonly prevHash?: string;
  readonly tsMs: number;
}

export interface AuditLog {
  /** Append an immutable, hash-chained audit entry. Returns the entry's chain hash. */
  record(entry: AuditEntry): Promise<string>;
  /** Verify the hash chain is intact (tamper-evidence). */
  verify(): Promise<boolean>;
  flush(): Promise<void>;
}

/** A persisted, chained record: the entry plus its derived chain hash. */
interface ChainedRecord {
  readonly seq: number;
  readonly hash: string;
  readonly entry: AuditEntry;
}

/** Genesis link for the first entry in an empty chain. */
export const GENESIS_HASH = "0".repeat(64);

/**
 * Canonical JSON: keys sorted recursively and bigints stringified, so the same
 * logical entry always hashes to the same bytes regardless of insertion order.
 */
export function canonicalize(value: unknown): string {
  return JSON.stringify(sortValue(value));
}

function sortValue(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString();
  if (Array.isArray(value)) return value.map(sortValue);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const k of Object.keys(value as Record<string, unknown>).sort()) {
      out[k] = sortValue((value as Record<string, unknown>)[k]);
    }
    return out;
  }
  return value;
}

/** Compute the chain hash for an entry linked to `prevHash`. */
export function chainHash(prevHash: string, entry: AuditEntry): string {
  const payload = canonicalize({
    event: entry.event,
    trackId: entry.trackId ?? null,
    sliceId: entry.sliceId ?? null,
    detail: entry.detail,
    tsMs: entry.tsMs,
    prevHash,
  });
  return createHash("sha256").update(payload).digest("hex");
}

/**
 * File-backed, hash-chained audit log. Each line of the JSONL file is one
 * `ChainedRecord`. The head hash is held in memory and advances on each
 * `record()`; on construction call `init()` (or the first `record`/`verify`)
 * to load the existing head so a restart continues the same chain.
 */
export class FileAuditLog implements AuditLog {
  private headHash: string = GENESIS_HASH;
  private seq = 0;
  private loaded = false;
  private writeChain: Promise<void> = Promise.resolve();

  constructor(private readonly filePath: string) {}

  /** Load the existing chain head so appends continue the same chain. */
  async init(): Promise<void> {
    if (this.loaded) return;
    await mkdir(dirname(this.filePath), { recursive: true });
    const records = await this.readAll();
    const last = records[records.length - 1];
    if (last) {
      this.headHash = last.hash;
      this.seq = last.seq + 1;
    }
    this.loaded = true;
  }

  async record(entry: AuditEntry): Promise<string> {
    if (!this.loaded) await this.init();
    const prevHash = this.headHash;
    const hash = chainHash(prevHash, entry);
    const record: ChainedRecord = {
      seq: this.seq,
      hash,
      entry: { ...entry, prevHash },
    };
    this.headHash = hash;
    this.seq += 1;
    // Serialize appends so concurrent callers can't interleave partial lines.
    this.writeChain = this.writeChain.then(() =>
      appendFile(this.filePath, `${JSON.stringify(record)}\n`, "utf8"),
    );
    await this.writeChain;
    return hash;
  }

  async verify(): Promise<boolean> {
    const records = await this.readAll();
    let prev = GENESIS_HASH;
    for (const rec of records) {
      const expected = chainHash(prev, rec.entry);
      if (expected !== rec.hash) return false;
      if ((rec.entry.prevHash ?? GENESIS_HASH) !== prev) return false;
      prev = rec.hash;
    }
    return true;
  }

  async flush(): Promise<void> {
    // appendFile resolves only after the data is handed to the OS; awaiting the
    // write chain guarantees every queued record has been flushed.
    await this.writeChain;
  }

  /** Read and parse all persisted records; returns [] when the file is absent. */
  private async readAll(): Promise<ChainedRecord[]> {
    let text: string;
    try {
      text = await readFile(this.filePath, "utf8");
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") return [];
      throw err;
    }
    return text
      .split("\n")
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line) as ChainedRecord);
  }
}

/**
 * In-memory audit log for tests and ephemeral runs. Same chaining semantics,
 * no file I/O. `entries()` exposes a defensive copy for assertions.
 */
export class MemoryAuditLog implements AuditLog {
  private readonly records: ChainedRecord[] = [];
  private headHash = GENESIS_HASH;

  async record(entry: AuditEntry): Promise<string> {
    const prevHash = this.headHash;
    const hash = chainHash(prevHash, entry);
    this.records.push({ seq: this.records.length, hash, entry: { ...entry, prevHash } });
    this.headHash = hash;
    return hash;
  }

  async verify(): Promise<boolean> {
    let prev = GENESIS_HASH;
    for (const rec of this.records) {
      if (chainHash(prev, rec.entry) !== rec.hash) return false;
      prev = rec.hash;
    }
    return true;
  }

  async flush(): Promise<void> {}

  /** Defensive copy of the chain for assertions. */
  entries(): readonly ChainedRecord[] {
    return this.records.map((r) => ({ ...r }));
  }
}
