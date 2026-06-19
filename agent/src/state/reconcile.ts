import type * as Casper from "casper-js-sdk";
import type { VaultState, VaultStatus } from "../types.js";
import type { DurableStateStore, TrackSnapshot } from "./store.js";
import { emptySnapshot } from "./store.js";

/**
 * On-chain reconciliation.
 *
 * The contract is the authority on economic state; the local store holds only an
 * optimistic cache plus a crash-recovery journal. On startup (and after any
 * crash recovery), {@link reconcileTrack} reads the authoritative {@link VaultState}
 * directly from the vault's global-state named keys and overwrites the local
 * snapshot, so the agent never resumes from stale optimistic numbers.
 *
 * Reads use `getStateRootHashLatest()` then `queryGlobalStateByStateHash(...)`
 * against the vault contract entity, exactly the primitives the dashboard uses.
 * Everything here returns new objects; nothing is mutated in place.
 */

/** The subset of `RpcClient` reconciliation needs — narrowed for testability. */
export interface VaultStateReader {
  getStateRootHashLatest(): Promise<{ stateRootHash: { toHex(): string } }>;
  queryGlobalStateByStateHash(
    stateRootHash: string | null,
    key: string,
    path: string[],
  ): Promise<Casper.QueryGlobalStateResult>;
}

/** Named keys the vault exposes for its progress state (Odra `Var` storage). */
export const VAULT_NAMED_KEYS = {
  status: "status",
  soldSoFar: "sold_so_far",
  boughtSoFar: "bought_so_far",
  sliceCount: "slice_count",
  totalSell: "total_sell",
} as const;

/** Odra encodes the `Status` enum as a u8 discriminant in this order. Must stay
 * in lockstep with `contracts/vault/src/vault/status.rs` (Halted is terminal,
 * set by `emergency_withdraw`). */
const STATUS_BY_DISCRIMINANT: readonly VaultStatus[] = [
  "Funded",
  "Active",
  "Paused",
  "Completed",
  "Expired",
  "Halted",
];

export interface ReconcileOptions {
  /** Contract entity key, e.g. "entity-contract-…" or "hash-…". */
  readonly contractKey: string;
  /** Block height the read was taken at, recorded as `lastSeenBlock`. */
  readonly blockHeight?: number;
}

/**
 * Numeric CLValues across SDK versions expose `toString()` (and `toNumber()`).
 * We read defensively against either to keep this resilient to the boxing the
 * RpcClient chooses for a given named key.
 */
interface NumericLike {
  toString(): string;
  toNumber?(): number;
}

/** First non-undefined numeric box on a queried CLValue (u8/u32/u512/u256/u64…). */
function numericOf(result: Casper.QueryGlobalStateResult): NumericLike | undefined {
  const cl = result.storedValue?.clValue as Record<string, NumericLike | undefined> | undefined;
  if (!cl) return undefined;
  return cl["ui8"] ?? cl["ui32"] ?? cl["ui64"] ?? cl["ui128"] ?? cl["ui256"] ?? cl["ui512"];
}

/** Extract a `bigint` from a queried unsigned-integer named key. */
function readU512(result: Casper.QueryGlobalStateResult, name: string): bigint {
  const n = numericOf(result);
  const s = n?.toString();
  if (s !== undefined && /^\d+$/.test(s)) return BigInt(s);
  throw new Error(`reconcile: named key '${name}' is not a readable U512`);
}

/** Extract a non-negative `number` from a queried U32 named key. */
function readU32(result: Casper.QueryGlobalStateResult, name: string): number {
  const n = numericOf(result);
  const s = n?.toString();
  if (s !== undefined && /^\d+$/.test(s)) return Number(s);
  throw new Error(`reconcile: named key '${name}' is not a readable U32`);
}

/** Extract the `Status` discriminant (u8) from a queried named key. */
function readStatus(result: Casper.QueryGlobalStateResult, name: string): VaultStatus {
  const n = numericOf(result);
  const s = n?.toString();
  if (s === undefined || !/^\d+$/.test(s)) {
    throw new Error(`reconcile: named key '${name}' is not a readable Status u8`);
  }
  const status = STATUS_BY_DISCRIMINANT[Number(s)];
  if (status === undefined) throw new Error(`reconcile: unknown Status discriminant ${s}`);
  return status;
}

/**
 * Read the authoritative {@link VaultState} for a vault from global state.
 *
 * Each named key is queried at the same state-root hash so the snapshot is
 * internally consistent (a single chain point-in-time), never a smeared read.
 */
export async function readOnChainVaultState(
  rpc: VaultStateReader,
  contractKey: string,
): Promise<VaultState> {
  const srh = (await rpc.getStateRootHashLatest()).stateRootHash.toHex();
  const query = (path: string): Promise<Casper.QueryGlobalStateResult> =>
    rpc.queryGlobalStateByStateHash(srh, contractKey, [path]);

  const [status, sold, bought, slices, total] = await Promise.all([
    query(VAULT_NAMED_KEYS.status),
    query(VAULT_NAMED_KEYS.soldSoFar),
    query(VAULT_NAMED_KEYS.boughtSoFar),
    query(VAULT_NAMED_KEYS.sliceCount),
    query(VAULT_NAMED_KEYS.totalSell),
  ]);

  return {
    status: readStatus(status, VAULT_NAMED_KEYS.status),
    soldSoFar: readU512(sold, VAULT_NAMED_KEYS.soldSoFar),
    boughtSoFar: readU512(bought, VAULT_NAMED_KEYS.boughtSoFar),
    sliceCount: readU32(slices, VAULT_NAMED_KEYS.sliceCount),
    totalSell: readU512(total, VAULT_NAMED_KEYS.totalSell),
  };
}

/**
 * Merge an authoritative on-chain {@link VaultState} into the persisted snapshot.
 *
 * On-chain state OVERWRITES the optimistic economic numbers; operational state
 * (breaker, price history) is preserved across the reconcile so the breaker's
 * hysteresis and the volatility estimate survive a restart. Returns a new
 * snapshot — the input is never mutated.
 */
export function mergeReconciled(
  prev: TrackSnapshot | null,
  trackId: string,
  onChain: VaultState,
  blockHeight?: number,
): TrackSnapshot {
  const base = prev ?? emptySnapshot(trackId, onChain.totalSell);
  return {
    ...base,
    trackId,
    state: onChain,
    ...(blockHeight !== undefined ? { lastSeenBlock: blockHeight } : {}),
    updatedAtMs: Date.now(),
  };
}

/**
 * Reconcile a single track against chain and persist the result.
 *
 * Returns the reconciled snapshot. Callers (loop / portfolio run) should treat
 * the returned `state` as the resume point, discarding any in-memory optimistic
 * state. Operational fields (breaker/priceHistory) are carried over from the
 * last persisted snapshot.
 */
export async function reconcileTrack(
  rpc: VaultStateReader,
  store: DurableStateStore,
  trackId: string,
  opts: ReconcileOptions,
): Promise<TrackSnapshot> {
  const onChain = await readOnChainVaultState(rpc, opts.contractKey);
  const prev = await store.loadSnapshot(trackId);
  const merged = mergeReconciled(prev, trackId, onChain, opts.blockHeight);
  await store.saveSnapshot(merged);
  return merged;
}

/**
 * Reconcile every track in a portfolio. Tracks are keyed by vault contract hash;
 * `contractKeyOf` maps a track id to its global-state entity key (they differ
 * when the entity prefix is "entity-contract-…" rather than the raw hash).
 *
 * Reconciliation is independent per track, so a single failed read does not
 * block the others — failures are collected so the loop can decide whether to
 * proceed with the tracks that did reconcile.
 */
export async function reconcileAll(
  rpc: VaultStateReader,
  store: DurableStateStore,
  trackIds: readonly string[],
  contractKeyOf: (trackId: string) => string,
): Promise<{
  readonly snapshots: ReadonlyMap<string, TrackSnapshot>;
  readonly failures: ReadonlyMap<string, Error>;
}> {
  const snapshots = new Map<string, TrackSnapshot>();
  const failures = new Map<string, Error>();
  await Promise.all(
    trackIds.map(async (trackId) => {
      try {
        const snap = await reconcileTrack(rpc, store, trackId, {
          contractKey: contractKeyOf(trackId),
        });
        snapshots.set(trackId, snap);
      } catch (err) {
        failures.set(trackId, err instanceof Error ? err : new Error(String(err)));
      }
    }),
  );
  return { snapshots, failures };
}
