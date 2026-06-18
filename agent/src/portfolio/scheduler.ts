import type { MandateTrack } from "./types.js";

export interface SchedulerInput {
  readonly tracks: readonly MandateTrack[];
  readonly nowMs: number;
}

/**
 * A track is actionable when it can take a slice right now: its vault is Active,
 * the order is not yet complete, and the execution window is still open. Mirrors
 * the conditions the vault's `execute_slice` enforces on-chain.
 */
export function isActionable(track: MandateTrack, nowMs: number): boolean {
  return (
    track.state.status === "Active" &&
    track.state.soldSoFar < track.mandate.totalSell &&
    nowMs <= track.mandate.endTimeMs
  );
}

/** Size still to sell on a track, in sell-asset base units. */
function remaining(track: MandateTrack): bigint {
  return track.mandate.totalSell - track.state.soldSoFar;
}

/** Milliseconds left until the deadline, floored at 1 to avoid divide-by-zero. */
function timeLeftMs(track: MandateTrack, nowMs: number): bigint {
  const left = BigInt(track.mandate.endTimeMs - nowMs);
  return left > 0n ? left : 1n;
}

/**
 * Whether `a` is under more time pressure than `b`: a higher required rate
 * (remaining size per millisecond left). Compared by exact bigint
 * cross-multiplication to avoid floating-point error. Ties break by the nearer
 * deadline, then by lexicographic id so selection is fully deterministic.
 */
function moreUrgent(a: MandateTrack, b: MandateTrack, nowMs: number): boolean {
  const lhs = remaining(a) * timeLeftMs(b, nowMs);
  const rhs = remaining(b) * timeLeftMs(a, nowMs);
  if (lhs !== rhs) return lhs > rhs;
  if (a.mandate.endTimeMs !== b.mandate.endTimeMs) return a.mandate.endTimeMs < b.mandate.endTimeMs;
  return a.id < b.id;
}

/**
 * Pick the next mandate to execute — the most time-pressured actionable track.
 * Returns null when nothing is actionable. Pure and deterministic: given the same
 * tracks and time it always returns the same track.
 */
export function selectNext(input: SchedulerInput): MandateTrack | null {
  const actionable = input.tracks.filter((t) => isActionable(t, input.nowMs));
  if (actionable.length === 0) return null;
  return actionable.reduce((best, t) => (moreUrgent(t, best, input.nowMs) ? t : best));
}
