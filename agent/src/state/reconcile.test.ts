import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileStateStore, emptySnapshot } from "./store.js";
import {
  VAULT_NAMED_KEYS,
  mergeReconciled,
  readOnChainVaultState,
  reconcileAll,
  reconcileTrack,
  resolveStartupState,
  type VaultStateReader,
} from "./reconcile.js";
import type { TrackSnapshot } from "./store.js";
import type { VaultState } from "../types.js";

/** A reader whose on-chain read always fails (node down / not found). */
function failingReader(): VaultStateReader {
  return {
    async getStateRootHashLatest() {
      throw new Error("rpc unreachable");
    },
    async queryGlobalStateByStateHash() {
      throw new Error("rpc unreachable");
    },
  };
}

/** A persisted snapshot carrying prior progress for the fail-closed test. */
function snapshotWithProgress(soldSoFar: bigint, sliceCount: number): TrackSnapshot {
  return {
    ...emptySnapshot("track-1", 100_000n),
    state: { status: "Active", soldSoFar, boughtSoFar: 0n, sliceCount, totalSell: 100_000n },
  };
}

/** Build a fake QueryGlobalStateResult exposing a single CLValue shape. */
function clResult(clValue: Record<string, unknown>): any {
  return { storedValue: { clValue } };
}

/**
 * A scripted reader returning per-named-key CLValues. Status is a u8
 * discriminant; amounts are U512 (ui512.toString()); counts are U32.
 */
function makeReader(values: {
  statusDiscriminant: number;
  soldSoFar: string;
  boughtSoFar: string;
  sliceCount: number;
  totalSell: string;
  blockSrh?: string;
}): VaultStateReader & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    async getStateRootHashLatest() {
      return { stateRootHash: { toHex: () => values.blockSrh ?? "srh-deadbeef" } };
    },
    async queryGlobalStateByStateHash(_srh, _key, path) {
      const name = path[0];
      calls.push(name as string);
      switch (name) {
        case VAULT_NAMED_KEYS.status:
          return clResult({ ui8: { toString: () => String(values.statusDiscriminant) } });
        case VAULT_NAMED_KEYS.soldSoFar:
          return clResult({ ui512: { toString: () => values.soldSoFar } });
        case VAULT_NAMED_KEYS.boughtSoFar:
          return clResult({ ui512: { toString: () => values.boughtSoFar } });
        case VAULT_NAMED_KEYS.sliceCount:
          return clResult({ ui32: { toString: () => String(values.sliceCount) } });
        case VAULT_NAMED_KEYS.totalSell:
          return clResult({ ui512: { toString: () => values.totalSell } });
        default:
          throw new Error(`unexpected named key ${name}`);
      }
    },
  };
}

describe("readOnChainVaultState", () => {
  it("reads all named keys at one state-root hash and decodes them", async () => {
    const reader = makeReader({
      statusDiscriminant: 1, // Active
      soldSoFar: "12345",
      boughtSoFar: "12000",
      sliceCount: 4,
      totalSell: "100000",
    });
    const state = await readOnChainVaultState(reader, "entity-contract-xyz");
    expect(state).toEqual<VaultState>({
      status: "Active",
      soldSoFar: 12_345n,
      boughtSoFar: 12_000n,
      sliceCount: 4,
      totalSell: 100_000n,
    });
    expect(new Set(reader.calls)).toEqual(new Set(Object.values(VAULT_NAMED_KEYS)));
  });

  it("decodes each Status discriminant correctly", async () => {
    const expected = ["Funded", "Active", "Paused", "Completed", "Expired"];
    for (let d = 0; d < expected.length; d++) {
      const reader = makeReader({
        statusDiscriminant: d,
        soldSoFar: "0",
        boughtSoFar: "0",
        sliceCount: 0,
        totalSell: "1",
      });
      const state = await readOnChainVaultState(reader, "k");
      expect(state.status).toBe(expected[d]);
    }
  });

  it("throws on an unknown status discriminant", async () => {
    const reader = makeReader({
      statusDiscriminant: 99,
      soldSoFar: "0",
      boughtSoFar: "0",
      sliceCount: 0,
      totalSell: "1",
    });
    await expect(readOnChainVaultState(reader, "k")).rejects.toThrow(/discriminant/);
  });

  it("throws when a U512 named key is unreadable", async () => {
    const reader: VaultStateReader = {
      async getStateRootHashLatest() {
        return { stateRootHash: { toHex: () => "srh" } };
      },
      async queryGlobalStateByStateHash(_srh, _key, path) {
        if (path[0] === VAULT_NAMED_KEYS.status) {
          return clResult({ ui8: { toString: () => "1" } });
        }
        return clResult({}); // no decodable value
      },
    };
    await expect(readOnChainVaultState(reader, "k")).rejects.toThrow(/U512/);
  });
});

describe("mergeReconciled", () => {
  const onChain: VaultState = {
    status: "Paused",
    soldSoFar: 7n,
    boughtSoFar: 6n,
    sliceCount: 2,
    totalSell: 50n,
  };

  it("overwrites economic state but preserves breaker and price history", () => {
    const prev = {
      ...emptySnapshot("t", 50n),
      breaker: { state: "open" as const, consecutiveFailures: 3, reason: "vol" },
      priceHistory: ["1", "2", "3"],
      seq: 5,
    };
    const merged = mergeReconciled(prev, "t", onChain, 99);
    expect(merged.state).toEqual(onChain);
    expect(merged.breaker).toEqual(prev.breaker);
    expect(merged.priceHistory).toEqual(["1", "2", "3"]);
    expect(merged.lastSeenBlock).toBe(99);
    expect(merged.seq).toBe(5);
    // prev not mutated
    expect(prev.state.soldSoFar).toBe(0n);
  });

  it("builds a fresh snapshot when none existed", () => {
    const merged = mergeReconciled(null, "t", onChain);
    expect(merged.state).toEqual(onChain);
    expect(merged.breaker).toEqual({ state: "closed", consecutiveFailures: 0 });
    expect(merged.lastSeenBlock).toBeUndefined();
  });
});

describe("reconcileTrack / reconcileAll", () => {
  let dir: string;
  let store: FileStateStore;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "cadence-reconcile-"));
    store = new FileStateStore(dir);
  });

  afterEach(async () => {
    await store.close();
    await rm(dir, { recursive: true, force: true });
  });

  it("reads chain, overwrites local optimistic state, and persists", async () => {
    // Persist a stale optimistic snapshot first.
    await store.saveState("vault-1", {
      status: "Active",
      soldSoFar: 999n, // optimistic / wrong
      boughtSoFar: 999n,
      sliceCount: 99,
      totalSell: 100_000n,
    });

    const reader = makeReader({
      statusDiscriminant: 1,
      soldSoFar: "200",
      boughtSoFar: "190",
      sliceCount: 1,
      totalSell: "100000",
    });
    const snap = await reconcileTrack(reader, store, "vault-1", { contractKey: "k", blockHeight: 7 });

    expect(snap.state.soldSoFar).toBe(200n); // authoritative wins
    expect(snap.lastSeenBlock).toBe(7);
    const persisted = await store.loadState("vault-1");
    expect(persisted?.soldSoFar).toBe(200n);
    expect(persisted?.sliceCount).toBe(1);
  });

  it("reconciles every track via reconcileAll and records snapshots", async () => {
    const reader = makeReader({
      statusDiscriminant: 3, // Completed
      soldSoFar: "10",
      boughtSoFar: "9",
      sliceCount: 1,
      totalSell: "10",
    });
    const { snapshots, failures } = await reconcileAll(reader, store, ["vault-a", "vault-b"], (id) => `key-${id}`);

    expect(failures.size).toBe(0);
    expect(snapshots.size).toBe(2);
    expect(snapshots.get("vault-a")?.state.status).toBe("Completed");
    expect((await store.loadState("vault-b"))?.soldSoFar).toBe(10n);
  });

  it("isolates a failing track in reconcileAll without aborting the others", async () => {
    let firstSrhCall = true;
    const flaky: VaultStateReader = {
      async getStateRootHashLatest() {
        // First track read fails; the global rpc is fine for the rest.
        if (firstSrhCall) {
          firstSrhCall = false;
          throw new Error("rpc down");
        }
        return { stateRootHash: { toHex: () => "srh" } };
      },
      async queryGlobalStateByStateHash(_srh, _key, path) {
        if (path[0] === VAULT_NAMED_KEYS.status) return clResult({ ui8: { toString: () => "1" } });
        if (path[0] === VAULT_NAMED_KEYS.sliceCount) return clResult({ ui32: { toString: () => "0" } });
        return clResult({ ui512: { toString: () => "1" } });
      },
    };
    const { snapshots, failures } = await reconcileAll(flaky, store, ["bad", "good"], (id) => id);
    expect(failures.size).toBe(1);
    expect(snapshots.size).toBe(1);
    expect([...failures.values()][0]?.message).toMatch(/rpc down/);
  });
});

describe("resolveStartupState", () => {
  it("seeds from chain when the on-chain read succeeds", async () => {
    const reader = makeReader({
      statusDiscriminant: 1, // Active
      soldSoFar: "500",
      boughtSoFar: "480",
      sliceCount: 3,
      totalSell: "100000",
    });
    const resolved = await resolveStartupState(reader, "key", null, 100_000n);
    expect(resolved.source).toBe("chain");
    expect(resolved.state.soldSoFar).toBe(500n);
    expect(resolved.state.sliceCount).toBe(3);
    expect(resolved.state.status).toBe("Active");
  });

  it("falls back to a fresh zero state on a cold start when the read fails", async () => {
    const resolved = await resolveStartupState(failingReader(), "key", null, 100_000n);
    expect(resolved.source).toBe("coldstart");
    expect(resolved.state).toEqual<VaultState>({
      status: "Active",
      soldSoFar: 0n,
      boughtSoFar: 0n,
      sliceCount: 0,
      totalSell: 100_000n,
    });
  });

  it("treats a zero-progress prior snapshot as a cold start when the read fails", async () => {
    const prev = snapshotWithProgress(0n, 0);
    const resolved = await resolveStartupState(failingReader(), "key", prev, 100_000n);
    expect(resolved.source).toBe("coldstart");
  });

  it("fails closed when the read fails and prior progress exists (sold > 0)", async () => {
    const prev = snapshotWithProgress(50n, 0);
    await expect(resolveStartupState(failingReader(), "key", prev, 100_000n)).rejects.toThrow(
      /refusing to resume/,
    );
  });

  it("fails closed when the read fails and prior slices exist", async () => {
    const prev = snapshotWithProgress(0n, 2);
    await expect(resolveStartupState(failingReader(), "key", prev, 100_000n)).rejects.toThrow(
      /refusing to resume/,
    );
  });
});
