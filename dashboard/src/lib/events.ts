import { PRICE_SCALE } from "@cadence/mandate";
import type { DashboardState, SliceView, VaultEvent, VaultStatus } from "../types.js";

/** Initial empty state before any event is observed. */
export function initialState(): DashboardState {
  return { status: "Unknown", totalSell: 0n, soldSoFar: 0n, boughtSoFar: 0n, slices: [] };
}

function upsertSlice(slices: SliceView[], id: number, patch: Partial<SliceView>): SliceView[] {
  const idx = slices.findIndex((s) => s.sliceId === id);
  if (idx === -1) {
    const base: SliceView = {
      sliceId: id,
      sellAmount: 0n,
      quotedOut: 0n,
      minOut: 0n,
      venue: "",
      status: "pending",
    };
    return [...slices, { ...base, ...patch }];
  }
  const next = slices.slice();
  next[idx] = { ...next[idx]!, ...patch };
  return next;
}

/** Fold a single on-chain event into the dashboard state. Pure. */
export function reduceEvent(state: DashboardState, ev: VaultEvent): DashboardState {
  switch (ev.kind) {
    case "MandateInitialised":
      return {
        ...state,
        status: "Funded",
        treasury: ev.treasury,
        agent: ev.agent,
        totalSell: BigInt(ev.totalSell),
        endTimeMs: ev.endTimeMs,
        maxSlippageBps: ev.maxSlippageBps,
      };
    case "VaultFunded":
      return { ...state, status: "Active" };
    case "SliceExecuted":
      return {
        ...state,
        soldSoFar: BigInt(ev.soldSoFar),
        slices: upsertSlice(state.slices, ev.sliceId, {
          sellAmount: BigInt(ev.sellAmount),
          quotedOut: BigInt(ev.quotedOut),
          minOut: BigInt(ev.minOut),
          venue: ev.venue,
          status: "pending",
          ...(ev.deployHash ? { sliceDeployHash: ev.deployHash } : {}),
          ...(ev.atMs !== undefined ? { atMs: ev.atMs } : {}),
        }),
      };
    case "FillRecorded":
      return {
        ...state,
        boughtSoFar: BigInt(ev.boughtSoFar),
        slices: upsertSlice(state.slices, ev.sliceId, {
          boughtAmount: BigInt(ev.boughtAmount),
          swapDeployHash: ev.swapDeployHash,
          status: "filled",
        }),
      };
    case "DecisionAttested":
      return { ...state, slices: upsertSlice(state.slices, ev.sliceId, { reason: ev.reason }) };
    case "StatusChanged":
      return { ...state, status: (ev.paused ? "Paused" : "Active") as VaultStatus };
    case "Settled":
      return {
        ...state,
        status: ev.completed ? "Completed" : "Expired",
        soldSoFar: BigInt(ev.soldSoFar),
        boughtSoFar: BigInt(ev.boughtSoFar),
        settled: {
          completed: ev.completed,
          sliceCount: ev.sliceCount,
          returnedToTreasury: BigInt(ev.returnedToTreasury),
        },
      };
    default:
      return state;
  }
}

export function reduceEvents(events: VaultEvent[]): DashboardState {
  return events.reduce(reduceEvent, initialState());
}

export interface Metrics {
  remaining: bigint;
  /** Realised average price (fixed point), or null until a slice fills. */
  averagePrice: bigint | null;
  /** Slippage saved vs the naive baseline in bps, or null when not computable. */
  slippageSavedBps: number | null;
  /** Milliseconds left in the window, or null if unknown. Clamped at 0. */
  timeLeftMs: number | null;
}

/**
 * Derive the headline metrics from reconstructed state. `naiveBaselinePrice` is
 * the fixed-point price a single naive market sell of the full size would realise
 * on the same pool; only when present can slippage-saved be shown.
 */
export function deriveMetrics(
  state: DashboardState,
  nowMs: number,
  naiveBaselinePrice: bigint | null,
): Metrics {
  const remaining = state.totalSell > state.soldSoFar ? state.totalSell - state.soldSoFar : 0n;
  const averagePrice =
    state.soldSoFar > 0n ? (state.boughtSoFar * PRICE_SCALE) / state.soldSoFar : null;

  let slippageSavedBps: number | null = null;
  if (averagePrice !== null && naiveBaselinePrice !== null && naiveBaselinePrice > 0n) {
    const diff = averagePrice - naiveBaselinePrice;
    slippageSavedBps = Number((diff * 10_000n) / naiveBaselinePrice);
  }

  const timeLeftMs =
    state.endTimeMs === undefined ? null : Math.max(0, state.endTimeMs - nowMs);

  return { remaining, averagePrice, slippageSavedBps, timeLeftMs };
}
