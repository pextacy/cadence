import type { VaultState } from "../types.js";
import type { MandateTrack } from "./types.js";
import { isActionable, selectNext } from "./scheduler.js";

/**
 * An immutable collection of mandate tracks under concurrent management. Every
 * mutation returns a new {@link Portfolio}; the instance is never modified in
 * place. The contract remains the authority on each vault's state — this is the
 * agent's local view used to decide which mandate to act on next.
 */
export class Portfolio {
  constructor(private readonly tracks: readonly MandateTrack[]) {}

  /** All tracks, in their original order. */
  list(): readonly MandateTrack[] {
    return this.tracks;
  }

  /** The track with the given id, or undefined. */
  get(id: string): MandateTrack | undefined {
    return this.tracks.find((t) => t.id === id);
  }

  /**
   * Return a new Portfolio with `id`'s state replaced. Throws if the id is
   * unknown — callers must only advance tracks the portfolio actually holds.
   */
  withTrackState(id: string, state: VaultState): Portfolio {
    if (this.get(id) === undefined) {
      throw new Error(`unknown mandate track: ${id}`);
    }
    return new Portfolio(this.tracks.map((t) => (t.id === id ? { ...t, state } : t)));
  }

  /** The next mandate to execute, or null when none is actionable. */
  selectNext(nowMs: number): MandateTrack | null {
    return selectNext({ tracks: this.tracks, nowMs });
  }

  /** True when no track can take another slice (all complete, paused, or closed). */
  allDone(nowMs: number): boolean {
    return this.tracks.every((t) => !isActionable(t, nowMs));
  }
}
