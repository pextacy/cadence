import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  FileStateStore,
  emptySnapshot,
  TERMINAL_STEPS,
  type SliceJournalRecord,
  type TrackSnapshot,
} from "./store.js";
import type { VaultState } from "../types.js";

const TRACK = "contract-abc123";

function record(over: Partial<SliceJournalRecord> = {}): SliceJournalRecord {
  return {
    trackId: TRACK,
    sliceId: 1,
    seq: 1,
    step: "proposed",
    sellAmount: "1000",
    minOut: "990",
    quotedOut: "1000",
    venue: "cspr.trade",
    reason: "twap",
    tsMs: 1_700_000_000_000,
    ...over,
  };
}

function vaultState(over: Partial<VaultState> = {}): VaultState {
  return {
    status: "Active",
    soldSoFar: 5_000n,
    boughtSoFar: 4_900n,
    sliceCount: 3,
    totalSell: 100_000n,
    ...over,
  };
}

describe("FileStateStore", () => {
  let dir: string;
  let store: FileStateStore;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "cadence-state-"));
    store = new FileStateStore(dir);
  });

  afterEach(async () => {
    await store.close();
    await rm(dir, { recursive: true, force: true });
  });

  it("appends journal records and replays them in order", async () => {
    await store.append(record({ seq: 1, step: "proposed" }));
    await store.append(record({ seq: 1, step: "slice_submitted", sliceTxHash: "tx1" }));
    await store.append(record({ seq: 1, step: "attested", attestTxHash: "tx2" }));

    const journal = await store.journal(TRACK);
    expect(journal.map((r) => r.step)).toEqual(["proposed", "slice_submitted", "attested"]);
    expect(journal[1]?.sliceTxHash).toBe("tx1");
    expect(journal[2]?.attestTxHash).toBe("tx2");
  });

  it("returns an empty journal for an unknown track", async () => {
    expect(await store.journal("nope")).toEqual([]);
  });

  it("round-trips a snapshot with bigints, breaker, and price history", async () => {
    const snap: TrackSnapshot = {
      ...emptySnapshot(TRACK, 100_000n),
      state: vaultState(),
      breaker: { state: "open", consecutiveFailures: 3, reason: "too volatile" },
      priceHistory: ["12345", "12350", "12360"],
      lastSeenBlock: 42,
      seq: 7,
      updatedAtMs: 1_700_000_000_000,
    };
    await store.saveSnapshot(snap);

    const loaded = await store.loadSnapshot(TRACK);
    expect(loaded).not.toBeNull();
    expect(loaded?.state.soldSoFar).toBe(5_000n);
    expect(loaded?.state.totalSell).toBe(100_000n);
    expect(loaded?.breaker).toEqual({ state: "open", consecutiveFailures: 3, reason: "too volatile" });
    expect(loaded?.priceHistory).toEqual(["12345", "12350", "12360"]);
    expect(loaded?.lastSeenBlock).toBe(42);
    expect(loaded?.seq).toBe(7);
  });

  it("loadState returns only the VaultState slice, or null when unknown", async () => {
    expect(await store.loadState(TRACK)).toBeNull();
    await store.saveState(TRACK, vaultState({ soldSoFar: 9n }));
    const s = await store.loadState(TRACK);
    expect(s?.soldSoFar).toBe(9n);
  });

  it("saveState preserves operational fields of an existing snapshot", async () => {
    await store.saveSnapshot({
      ...emptySnapshot(TRACK, 100_000n),
      breaker: { state: "open", consecutiveFailures: 2, reason: "streak" },
      priceHistory: ["1", "2"],
      lastSeenBlock: 10,
      seq: 4,
    });
    await store.saveState(TRACK, vaultState({ boughtSoFar: 123n }));

    const snap = await store.loadSnapshot(TRACK);
    expect(snap?.state.boughtSoFar).toBe(123n);
    expect(snap?.breaker.state).toBe("open");
    expect(snap?.priceHistory).toEqual(["1", "2"]);
    expect(snap?.lastSeenBlock).toBe(10);
    expect(snap?.seq).toBe(4);
  });

  it("reports only non-terminal slices as in-flight, latest step per slice", async () => {
    // slice 1: ends attested -> NOT in flight
    await store.append(record({ sliceId: 1, seq: 1, step: "slice_submitted" }));
    await store.append(record({ sliceId: 1, seq: 1, step: "attested" }));
    // slice 2: stuck at swap_submitted -> in flight
    await store.append(record({ sliceId: 2, seq: 2, step: "slice_confirmed" }));
    await store.append(record({ sliceId: 2, seq: 2, step: "swap_submitted", swapDeployHash: "d2" }));
    // slice 3: aborted -> NOT in flight
    await store.append(record({ sliceId: 3, seq: 3, step: "aborted" }));

    const inFlight = await store.inFlight();
    expect(inFlight).toHaveLength(1);
    expect(inFlight[0]?.sliceId).toBe(2);
    expect(inFlight[0]?.step).toBe("swap_submitted");
    expect(inFlight[0]?.swapDeployHash).toBe("d2");
  });

  it("aggregates in-flight slices across multiple tracks", async () => {
    await store.append(record({ trackId: "vault-A", sliceId: 1, step: "slice_submitted" }));
    await store.append(record({ trackId: "vault-B", sliceId: 1, step: "attested" }));
    const inFlight = await store.inFlight();
    expect(inFlight.map((r) => r.trackId)).toEqual(["vault-A"]);
  });

  it("computes the high-water sequence across journal and snapshots", async () => {
    await store.append(record({ seq: 2 }));
    await store.append(record({ seq: 5 }));
    await store.saveSnapshot({ ...emptySnapshot(TRACK, 1n), seq: 3 });
    expect(await store.highWaterSeq()).toBe(5);
  });

  it("rejects a corrupt snapshot with a typed error", async () => {
    await store.saveSnapshot({ ...emptySnapshot(TRACK, 1n) });
    const { writeFile } = await import("node:fs/promises");
    await writeFile(
      join(dir, `state-${TRACK}.json`),
      JSON.stringify({
        trackId: TRACK,
        state: { status: "Bogus", soldSoFar: "0", boughtSoFar: "0", sliceCount: 0, totalSell: "1" },
        breaker: { state: "closed", consecutiveFailures: 0 },
        priceHistory: [],
        seq: 0,
        updatedAtMs: 0,
      }),
    );
    await expect(store.loadSnapshot(TRACK)).rejects.toThrow(/status/);
  });

  it("survives a fresh store instance reading prior writes (durability)", async () => {
    await store.append(record({ sliceId: 1, step: "swap_submitted" }));
    await store.saveState(TRACK, vaultState({ sliceCount: 9 }));
    await store.flush();

    const reopened = new FileStateStore(dir);
    expect((await reopened.loadState(TRACK))?.sliceCount).toBe(9);
    expect(await reopened.inFlight()).toHaveLength(1);
    await reopened.close();
  });

  it("serialises concurrent appends without interleaving", async () => {
    await Promise.all(
      Array.from({ length: 20 }, (_, i) => store.append(record({ sliceId: i, seq: i, step: "proposed" }))),
    );
    const journal = await store.journal(TRACK);
    expect(journal).toHaveLength(20);
    // Every line parsed cleanly (no torn writes).
    expect(new Set(journal.map((r) => r.sliceId)).size).toBe(20);
  });
});

describe("TERMINAL_STEPS", () => {
  it("marks attested and aborted as terminal only", () => {
    expect(TERMINAL_STEPS.has("attested")).toBe(true);
    expect(TERMINAL_STEPS.has("aborted")).toBe(true);
    expect(TERMINAL_STEPS.has("swap_submitted")).toBe(false);
    expect(TERMINAL_STEPS.has("proposed")).toBe(false);
  });
});
