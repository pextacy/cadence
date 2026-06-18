import type { DashboardState } from "../types.js";
import type { Metrics } from "../lib/events.js";
import { formatAmount, formatDuration } from "../lib/format.js";

/** Fraction in [0,1] of a / b, guarded against zero. */
function frac(a: bigint, b: bigint): number {
  if (b <= 0n) return 0;
  const n = Number((a * 10_000n) / b) / 10_000;
  return Math.max(0, Math.min(1, n));
}

/**
 * The Cadence Stave — Cadence's signature instrument. Two aligned lines read
 * together: the **volume cadence** (each executed slice is a measured beat whose
 * width is its share of the parent order, accumulating toward the cap) and the
 * **time cadence** (elapsed vs the deadline, with a live "now" marker and a tick
 * for every beat). Together they answer one question a treasurer cares about:
 * is the agent on tempo, or behind? Everything here is derived from real on-chain
 * events — no sample data.
 */
export function CadenceStave({
  state,
  metrics,
  sellAsset,
  nowMs,
  compact = false,
}: {
  state: DashboardState;
  metrics: Metrics;
  sellAsset: string;
  nowMs: number;
  compact?: boolean;
}): JSX.Element {
  const fillFrac = frac(state.soldSoFar, state.totalSell);

  // Time window: anchor the start at the earliest observed beat (or now if none
  // yet); the deadline is the mandate end. We never invent a start time.
  const beatTimes = state.slices
    .map((s) => s.atMs)
    .filter((t): t is number => typeof t === "number");
  const windowStart = beatTimes.length > 0 ? Math.min(...beatTimes, nowMs) : nowMs;
  const windowEnd = state.endTimeMs;
  const hasWindow = windowEnd !== undefined && windowEnd > windowStart;
  const elapsedFrac = hasWindow
    ? Math.max(0, Math.min(1, (nowMs - windowStart) / (windowEnd - windowStart)))
    : null;

  const onTempo =
    elapsedFrac === null ? null : fillFrac + 1e-9 >= elapsedFrac;

  const placedBeats = state.slices.filter((s) => s.sellAmount > 0n);

  return (
    <div className="stave">
      <div className="stave-head">
        <div>
          <span className="eyebrow">Cadence stave</span>
          <h2 style={{ fontSize: 17, marginTop: 6 }}>Execution rhythm</h2>
        </div>
        {onTempo !== null && (
          <span className={`badge ${onTempo ? "ok" : "hold"}`}>
            <span className="dot" />
            {onTempo ? "On tempo" : "Behind tempo"}
          </span>
        )}
      </div>

      {/* Volume cadence */}
      <div className="stave-line">
        <div className="cap">
          <span>Volume cadence</span>
          <span>{(fillFrac * 100).toFixed(1)}% of cap filled</span>
        </div>
        <div className="vol-track" role="img" aria-label={`${placedBeats.length} slices executed, ${(fillFrac * 100).toFixed(1)} percent of the cap filled`}>
          {placedBeats.map((s) => (
            <div
              key={s.sliceId}
              className={`beat-seg ${s.status === "filled" ? "ok" : s.status === "blocked" ? "blocked" : "pending"} appear`}
              style={{ width: `${frac(s.sellAmount, state.totalSell) * 100}%` }}
              title={`Slice ${s.sliceId}: ${formatAmount(s.sellAmount, sellAsset)} ${sellAsset} · ${s.status}`}
            />
          ))}
          <div className="vol-remaining" />
        </div>
      </div>

      {/* Time cadence */}
      <div className="stave-line">
        <div className="cap">
          <span>Time cadence</span>
          <span>
            {metrics.timeLeftMs !== null ? `${formatDuration(metrics.timeLeftMs)} left` : "deadline unknown"}
          </span>
        </div>
        <div className="time-track" role="img" aria-label="Elapsed time versus the mandate deadline">
          {elapsedFrac !== null && (
            <>
              <div className="time-elapsed" style={{ width: `${elapsedFrac * 100}%` }} />
              {hasWindow &&
                beatTimes.map((t, i) => (
                  <div
                    key={i}
                    className="beat-tick"
                    style={{ left: `${((t - windowStart) / (windowEnd! - windowStart)) * 100}%` }}
                  />
                ))}
              <div className="time-now" style={{ left: `${elapsedFrac * 100}%` }} />
            </>
          )}
        </div>
      </div>

      {!compact && (
        <div className="stave-readout">
          <div className="readout">
            <div className="k">Beats placed</div>
            <div className="v">{placedBeats.length}</div>
          </div>
          <div className="readout">
            <div className="k">Filled</div>
            <div className="v">{formatAmount(state.soldSoFar, sellAsset)}</div>
          </div>
          <div className="readout">
            <div className="k">Remaining</div>
            <div className="v">{formatAmount(metrics.remaining, sellAsset)}</div>
          </div>
          {elapsedFrac !== null && (
            <div className="readout">
              <div className="k">Window elapsed</div>
              <div className="v">{(elapsedFrac * 100).toFixed(0)}%</div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
