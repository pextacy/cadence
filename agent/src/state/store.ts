import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";
import type { VaultState, VaultStatus } from "../types.js";
import type { BreakerSnapshot } from "../executor/circuit-breaker/breaker.js";

/**
 * Durable state layer for the agent.
 *
 * The on-chain vault is the authority on economic state; this store is the
 * agent's crash-recovery journal and optimistic-state cache. It persists, per
 * track (keyed by vault contract hash):
 *   - an append-only journal of every slice-lifecycle transition,
 *   - the latest derived/reconciled {@link VaultState},
 *   - circuit-breaker state, recent price history, and the last-seen block.
 *
 * The default implementation is a JSONL file store with atomic writes (no new
 * dependency). The interface is storage-agnostic so a SQLite/Postgres backing
 * can be dropped in later without touching the loop. All reads/writes return new
 * objects; nothing is mutated in place.
 */

/** Lifecycle step of a single slice, ordered as the executor advances it. */
export type SliceStep =
  | "proposed"
  | "slice_submitted"
  | "slice_confirmed"
  | "swap_submitted"
  | "fill_recorded"
  | "attested"
  | "aborted";

/** Steps after which a slice is considered safely terminal (not in-flight). */
export const TERMINAL_STEPS: ReadonlySet<SliceStep> = new Set<SliceStep>([
  "attested",
  "aborted",
]);

/**
 * One append-only record per slice-lifecycle transition. `bigint` amounts are
 * serialised as decimal strings so the journal round-trips through JSON losslessly.
 */
export interface SliceJournalRecord {
  readonly trackId: string;
  readonly sliceId: number;
  /** Monotonic client sequence from the NonceManager; idempotency key for retries. */
  readonly seq: number;
  readonly step: SliceStep;
  readonly sellAmount: string;
  readonly minOut: string;
  readonly quotedOut: string;
  readonly venue: string;
  readonly sliceTxHash?: string;
  readonly swapDeployHash?: string;
  readonly boughtAmount?: string;
  readonly attestTxHash?: string;
  readonly reason: string;
  readonly tsMs: number;
}

/**
 * The full persisted per-track snapshot. Extends the spec's authoritative
 * {@link VaultState} with the operational state the loop must survive a restart:
 * circuit-breaker snapshot, recent mid-price history, and the last-seen block
 * (so reconciliation can skip already-processed history).
 */
export interface TrackSnapshot {
  readonly trackId: string;
  readonly state: VaultState;
  readonly breaker: BreakerSnapshot;
  /** Recent mid-price samples (fixed point), oldest first; decimal strings. */
  readonly priceHistory: readonly string[];
  /** Highest block height the agent has reconciled against, if any. */
  readonly lastSeenBlock?: number;
  /** High-water client sequence at the time of persistence (NonceManager restore). */
  readonly seq: number;
  readonly updatedAtMs: number;
}

export interface StateStore {
  /** Append an immutable journal record for a slice-lifecycle step. */
  append(record: SliceJournalRecord): Promise<void>;
  /** Latest persisted authoritative {@link VaultState} for a track, or null if unknown. */
  loadState(trackId: string): Promise<VaultState | null>;
  /** Persist the latest derived/reconciled {@link VaultState} for a track (atomic write). */
  saveState(trackId: string, state: VaultState): Promise<void>;
  /** All in-flight slices (submitted but not yet attested/aborted) across tracks. */
  inFlight(): Promise<readonly SliceJournalRecord[]>;
  /** Full ordered journal for a track (for audit/reconstruction). */
  journal(trackId: string): Promise<readonly SliceJournalRecord[]>;
  flush(): Promise<void>;
  close(): Promise<void>;
}

/**
 * Extended persistence the durable store also exposes for the operational layer
 * (breaker / price history / last-seen block). Kept separate from the locked
 * {@link StateStore} surface so consumers that only need the spec contract are
 * unaffected.
 */
export interface DurableStateStore extends StateStore {
  /** Load the full operational snapshot for a track, or null if unknown. */
  loadSnapshot(trackId: string): Promise<TrackSnapshot | null>;
  /** Persist the full operational snapshot for a track (atomic write). */
  saveSnapshot(snapshot: TrackSnapshot): Promise<void>;
  /** Highest client sequence seen across all tracks (NonceManager high-water). */
  highWaterSeq(): Promise<number>;
}

const VAULT_STATUSES: ReadonlySet<string> = new Set<VaultStatus>([
  "Funded",
  "Active",
  "Paused",
  "Completed",
  "Expired",
  "Halted",
]);

/** Build the default empty snapshot for an unknown track. */
export function emptySnapshot(trackId: string, totalSell: bigint): TrackSnapshot {
  return {
    trackId,
    state: {
      status: "Active",
      soldSoFar: 0n,
      boughtSoFar: 0n,
      sliceCount: 0,
      totalSell,
    },
    breaker: { state: "closed", consecutiveFailures: 0 },
    priceHistory: [],
    seq: 0,
    updatedAtMs: 0,
  };
}

// --- serialisation helpers (bigint <-> decimal string) -----------------------

function serialiseState(state: VaultState): Record<string, unknown> {
  return {
    status: state.status,
    soldSoFar: state.soldSoFar.toString(),
    boughtSoFar: state.boughtSoFar.toString(),
    sliceCount: state.sliceCount,
    totalSell: state.totalSell.toString(),
  };
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function reqString(v: unknown, field: string): string {
  if (typeof v !== "string") throw new Error(`state-store: ${field} must be a string`);
  return v;
}

function reqBigint(v: unknown, field: string): bigint {
  if (typeof v !== "string") throw new Error(`state-store: ${field} must be a numeric string`);
  try {
    return BigInt(v);
  } catch {
    throw new Error(`state-store: ${field} is not a valid integer: ${v}`);
  }
}

function reqNumber(v: unknown, field: string): number {
  if (typeof v !== "number" || !Number.isFinite(v)) {
    throw new Error(`state-store: ${field} must be a finite number`);
  }
  return v;
}

function parseState(raw: unknown): VaultState {
  if (!isRecord(raw)) throw new Error("state-store: state must be an object");
  const status = reqString(raw["status"], "status");
  if (!VAULT_STATUSES.has(status)) throw new Error(`state-store: unknown status ${status}`);
  return {
    status: status as VaultStatus,
    soldSoFar: reqBigint(raw["soldSoFar"], "soldSoFar"),
    boughtSoFar: reqBigint(raw["boughtSoFar"], "boughtSoFar"),
    sliceCount: reqNumber(raw["sliceCount"], "sliceCount"),
    totalSell: reqBigint(raw["totalSell"], "totalSell"),
  };
}

function serialiseSnapshot(s: TrackSnapshot): Record<string, unknown> {
  return {
    trackId: s.trackId,
    state: serialiseState(s.state),
    breaker: s.breaker,
    priceHistory: [...s.priceHistory],
    ...(s.lastSeenBlock !== undefined ? { lastSeenBlock: s.lastSeenBlock } : {}),
    seq: s.seq,
    updatedAtMs: s.updatedAtMs,
  };
}

function parseBreaker(raw: unknown): BreakerSnapshot {
  if (!isRecord(raw)) throw new Error("state-store: breaker must be an object");
  const state = reqString(raw["state"], "breaker.state");
  if (state !== "closed" && state !== "open") {
    throw new Error(`state-store: unknown breaker state ${state}`);
  }
  const snap: BreakerSnapshot = {
    state,
    consecutiveFailures: reqNumber(raw["consecutiveFailures"], "breaker.consecutiveFailures"),
    ...(typeof raw["reason"] === "string" ? { reason: raw["reason"] } : {}),
  };
  return snap;
}

function parseSnapshot(raw: unknown): TrackSnapshot {
  if (!isRecord(raw)) throw new Error("state-store: snapshot must be an object");
  const history = raw["priceHistory"];
  if (!Array.isArray(history)) throw new Error("state-store: priceHistory must be an array");
  return {
    trackId: reqString(raw["trackId"], "trackId"),
    state: parseState(raw["state"]),
    breaker: parseBreaker(raw["breaker"]),
    priceHistory: history.map((h, i) => reqString(h, `priceHistory[${i}]`)),
    ...(raw["lastSeenBlock"] !== undefined
      ? { lastSeenBlock: reqNumber(raw["lastSeenBlock"], "lastSeenBlock") }
      : {}),
    seq: reqNumber(raw["seq"], "seq"),
    updatedAtMs: reqNumber(raw["updatedAtMs"], "updatedAtMs"),
  };
}

function parseJournalRecord(raw: unknown): SliceJournalRecord {
  if (!isRecord(raw)) throw new Error("state-store: journal record must be an object");
  const step = reqString(raw["step"], "step") as SliceStep;
  const rec: SliceJournalRecord = {
    trackId: reqString(raw["trackId"], "trackId"),
    sliceId: reqNumber(raw["sliceId"], "sliceId"),
    seq: reqNumber(raw["seq"], "seq"),
    step,
    sellAmount: reqString(raw["sellAmount"], "sellAmount"),
    minOut: reqString(raw["minOut"], "minOut"),
    quotedOut: reqString(raw["quotedOut"], "quotedOut"),
    venue: reqString(raw["venue"], "venue"),
    reason: reqString(raw["reason"], "reason"),
    tsMs: reqNumber(raw["tsMs"], "tsMs"),
    ...(typeof raw["sliceTxHash"] === "string" ? { sliceTxHash: raw["sliceTxHash"] } : {}),
    ...(typeof raw["swapDeployHash"] === "string" ? { swapDeployHash: raw["swapDeployHash"] } : {}),
    ...(typeof raw["boughtAmount"] === "string" ? { boughtAmount: raw["boughtAmount"] } : {}),
    ...(typeof raw["attestTxHash"] === "string" ? { attestTxHash: raw["attestTxHash"] } : {}),
  };
  return rec;
}

/** File-system-safe encoding of a track id (vault contract hash) for file names. */
function safeTrackFile(trackId: string): string {
  return trackId.replace(/[^a-zA-Z0-9._-]/g, "_");
}

/**
 * Atomically write `text` to `path` via a temp file + rename. On POSIX rename is
 * atomic within a filesystem, so a crash mid-write never leaves a torn snapshot.
 */
async function atomicWrite(path: string, text: string): Promise<void> {
  const tmp = `${path}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(tmp, text, "utf8");
  await rename(tmp, path);
}

/**
 * JSONL file-backed durable store.
 *
 * Layout under `baseDir`:
 *   journal-<track>.jsonl   append-only, one JSON record per line
 *   state-<track>.json      latest atomic snapshot (state + breaker + history)
 *
 * The journal is the source of truth for crash recovery; the snapshot is a fast
 * read cache for the loop. Both are rebuilt by replaying the journal if needed.
 */
export class FileStateStore implements DurableStateStore {
  private readonly baseDir: string;
  /** Per-track serialisation queue so concurrent appends/saves never interleave. */
  private readonly locks = new Map<string, Promise<void>>();
  /** Known track ids (vault hashes), discovered lazily from disk or registration. */
  private readonly tracks = new Set<string>();

  constructor(baseDir: string) {
    this.baseDir = baseDir;
  }

  private journalPath(trackId: string): string {
    return join(this.baseDir, `journal-${safeTrackFile(trackId)}.jsonl`);
  }

  private statePath(trackId: string): string {
    return join(this.baseDir, `state-${safeTrackFile(trackId)}.json`);
  }

  /** Serialise writes per track id so file operations never race each other. */
  private async withLock<T>(trackId: string, fn: () => Promise<T>): Promise<T> {
    const prev = this.locks.get(trackId) ?? Promise.resolve();
    let release!: () => void;
    const next = new Promise<void>((r) => (release = r));
    this.locks.set(
      trackId,
      prev.then(() => next),
    );
    await prev;
    try {
      return await fn();
    } finally {
      release();
    }
  }

  private async ensureDir(): Promise<void> {
    if (!existsSync(this.baseDir)) {
      await mkdir(this.baseDir, { recursive: true });
    }
  }

  async append(record: SliceJournalRecord): Promise<void> {
    await this.ensureDir();
    this.tracks.add(record.trackId);
    const line = `${JSON.stringify(record)}\n`;
    await this.withLock(record.trackId, async () => {
      // Append is atomic for a single small line on POSIX; flag 'a' opens O_APPEND.
      await writeFile(this.journalPath(record.trackId), line, { flag: "a" });
    });
  }

  async journal(trackId: string): Promise<readonly SliceJournalRecord[]> {
    const path = this.journalPath(trackId);
    if (!existsSync(path)) return [];
    const text = await readFile(path, "utf8");
    const out: SliceJournalRecord[] = [];
    for (const line of text.split("\n")) {
      const trimmed = line.trim();
      if (trimmed.length === 0) continue;
      out.push(parseJournalRecord(JSON.parse(trimmed)));
    }
    return out;
  }

  async inFlight(): Promise<readonly SliceJournalRecord[]> {
    const tracks = await this.discoverTracks();
    const latestBySlice = new Map<string, SliceJournalRecord>();
    for (const trackId of tracks) {
      const records = await this.journal(trackId);
      for (const rec of records) {
        // The last record per (track, slice) wins — it is the slice's latest step.
        latestBySlice.set(`${rec.trackId}#${rec.sliceId}`, rec);
      }
    }
    return [...latestBySlice.values()].filter((r) => !TERMINAL_STEPS.has(r.step));
  }

  async loadSnapshot(trackId: string): Promise<TrackSnapshot | null> {
    const path = this.statePath(trackId);
    if (!existsSync(path)) return null;
    const text = await readFile(path, "utf8");
    return parseSnapshot(JSON.parse(text));
  }

  async loadState(trackId: string): Promise<VaultState | null> {
    const snap = await this.loadSnapshot(trackId);
    return snap?.state ?? null;
  }

  async saveSnapshot(snapshot: TrackSnapshot): Promise<void> {
    await this.ensureDir();
    this.tracks.add(snapshot.trackId);
    const text = `${JSON.stringify(serialiseSnapshot(snapshot), null, 2)}\n`;
    await this.withLock(snapshot.trackId, async () => {
      await atomicWrite(this.statePath(snapshot.trackId), text);
    });
  }

  async saveState(trackId: string, state: VaultState): Promise<void> {
    const existing = await this.loadSnapshot(trackId);
    const next: TrackSnapshot = existing
      ? { ...existing, state, updatedAtMs: Date.now() }
      : { ...emptySnapshot(trackId, state.totalSell), state, updatedAtMs: Date.now() };
    await this.saveSnapshot(next);
  }

  async highWaterSeq(): Promise<number> {
    const tracks = await this.discoverTracks();
    let hi = 0;
    for (const trackId of tracks) {
      const snap = await this.loadSnapshot(trackId);
      if (snap && snap.seq > hi) hi = snap.seq;
      for (const rec of await this.journal(trackId)) {
        if (rec.seq > hi) hi = rec.seq;
      }
    }
    return hi;
  }

  /** Union of in-memory registered tracks and any track files already on disk. */
  private async discoverTracks(): Promise<readonly string[]> {
    if (!existsSync(this.baseDir)) return [...this.tracks];
    const { readdir } = await import("node:fs/promises");
    const entries = await readdir(this.baseDir);
    const found = new Set<string>(this.tracks);
    for (const name of entries) {
      const m = name.match(/^journal-(.+)\.jsonl$/);
      if (m) {
        // We index by the safe-encoded id; re-derivation is not lossless, so we
        // keep the registered (real) ids and only add unseen encoded ones.
        const encoded = m[1] as string;
        if (![...found].some((t) => safeTrackFile(t) === encoded)) found.add(encoded);
      }
    }
    return [...found];
  }

  /** No-op: every write is flushed synchronously (atomic rename / O_APPEND). */
  async flush(): Promise<void> {
    // Wait for any outstanding per-track locks to drain.
    await Promise.all([...this.locks.values()]);
  }

  async close(): Promise<void> {
    await this.flush();
  }
}

/** Resolve the default state directory under a base path (config-driven). */
export function defaultStateDir(base = ".cadence-state"): string {
  return base;
}
