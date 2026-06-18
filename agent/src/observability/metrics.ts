/**
 * MetricsSink: counters, gauges and histograms for the agent's operational
 * surface — slices filled/skipped/paused, confirmation latency, LLM tokens,
 * breaker trips, venue health. The default impl is in-process and renders
 * Prometheus text exposition for the health endpoint (`/metrics`). No new dep.
 *
 * All public methods are non-throwing: observability must never crash the loop.
 * The sink owns mutable accumulator maps internally (a metrics registry is
 * inherently stateful), but `render()` produces a fresh string snapshot and
 * never exposes internal references.
 */

/** Immutable label set attached to a metric sample. */
export type Labels = Readonly<Record<string, string>>;

export interface MetricsSink {
  /** Increment a counter by `value` (default 1). Counters only go up. */
  inc(metric: string, value?: number, labels?: Labels): void;
  /** Set a gauge to an absolute `value` (can go up or down). */
  gauge(metric: string, value: number, labels?: Labels): void;
  /** Record an observation into a histogram (e.g. a latency in ms). */
  observe(metric: string, value: number, labels?: Labels): void;
  /** Render current metrics as Prometheus text exposition for `/metrics`. */
  render(): string;
}

/** Default histogram buckets (milliseconds) tuned for confirmation latency. */
export const DEFAULT_BUCKETS_MS: readonly number[] = [
  50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000, 180_000,
];

const METRIC_NAME_RE = /^[a-zA-Z_:][a-zA-Z0-9_:]*$/;
const LABEL_NAME_RE = /^[a-zA-Z_][a-zA-Z0-9_]*$/;

/** Build a stable series key from a metric name and its (sorted) labels. */
function seriesKey(metric: string, labels: Labels | undefined): string {
  if (!labels) return metric;
  const parts = Object.keys(labels)
    .filter((k) => LABEL_NAME_RE.test(k))
    .sort()
    .map((k) => `${k}=${labels[k] ?? ""}`);
  return parts.length === 0 ? metric : `${metric}{${parts.join(",")}}`;
}

/** Escape a Prometheus label value per the text exposition format. */
function escapeLabelValue(v: string): string {
  return v.replace(/\\/g, "\\\\").replace(/\n/g, "\\n").replace(/"/g, '\\"');
}

/** Render the `{a="1",b="2"}` label clause for a metric line. */
function renderLabels(labels: Labels | undefined): string {
  if (!labels) return "";
  const parts = Object.keys(labels)
    .filter((k) => LABEL_NAME_RE.test(k))
    .sort()
    .map((k) => `${k}="${escapeLabelValue(labels[k] ?? "")}"`);
  return parts.length === 0 ? "" : `{${parts.join(",")}}`;
}

interface CounterSample {
  readonly kind: "counter";
  readonly metric: string;
  readonly labels?: Labels;
  value: number;
}
interface GaugeSample {
  readonly kind: "gauge";
  readonly metric: string;
  readonly labels?: Labels;
  value: number;
}
interface HistogramSample {
  readonly kind: "histogram";
  readonly metric: string;
  readonly labels?: Labels;
  readonly counts: number[];
  sum: number;
  count: number;
}

type Sample = CounterSample | GaugeSample | HistogramSample;

/**
 * In-process metrics registry. Frequencies are aggregated in memory and
 * rendered on demand. Safe to share across portfolio tracks (single-threaded
 * Node event loop). Unknown/invalid metric names are dropped silently rather
 * than throwing, so a typo can never take down the trading loop.
 */
export class InProcessMetrics implements MetricsSink {
  private readonly samples = new Map<string, Sample>();
  private readonly buckets: readonly number[];

  constructor(buckets: readonly number[] = DEFAULT_BUCKETS_MS) {
    this.buckets = [...buckets].sort((a, b) => a - b);
  }

  inc(metric: string, value = 1, labels?: Labels): void {
    if (!METRIC_NAME_RE.test(metric) || !Number.isFinite(value) || value < 0) return;
    const key = seriesKey(metric, labels);
    const existing = this.samples.get(key);
    if (existing && existing.kind === "counter") {
      existing.value += value;
      return;
    }
    if (existing) return; // type clash — ignore
    this.samples.set(key, { kind: "counter", metric, ...(labels ? { labels } : {}), value });
  }

  gauge(metric: string, value: number, labels?: Labels): void {
    if (!METRIC_NAME_RE.test(metric) || !Number.isFinite(value)) return;
    const key = seriesKey(metric, labels);
    const existing = this.samples.get(key);
    if (existing && existing.kind === "gauge") {
      existing.value = value;
      return;
    }
    if (existing) return;
    this.samples.set(key, { kind: "gauge", metric, ...(labels ? { labels } : {}), value });
  }

  observe(metric: string, value: number, labels?: Labels): void {
    if (!METRIC_NAME_RE.test(metric) || !Number.isFinite(value) || value < 0) return;
    const key = seriesKey(metric, labels);
    let existing = this.samples.get(key);
    if (existing && existing.kind !== "histogram") return;
    if (!existing) {
      const hist: HistogramSample = {
        kind: "histogram",
        metric,
        ...(labels ? { labels } : {}),
        counts: new Array(this.buckets.length).fill(0),
        sum: 0,
        count: 0,
      };
      this.samples.set(key, hist);
      existing = hist;
    }
    const hist = existing as HistogramSample;
    hist.sum += value;
    hist.count += 1;
    for (let i = 0; i < this.buckets.length; i += 1) {
      const bound = this.buckets[i];
      if (bound !== undefined && value <= bound) hist.counts[i] = (hist.counts[i] ?? 0) + 1;
    }
  }

  render(): string {
    const lines: string[] = [];
    // Group by metric name so HELP/TYPE headers are emitted once per family.
    const byMetric = new Map<string, Sample[]>();
    for (const s of this.samples.values()) {
      const arr = byMetric.get(s.metric) ?? [];
      arr.push(s);
      byMetric.set(s.metric, arr);
    }
    for (const [metric, group] of [...byMetric.entries()].sort()) {
      const first = group[0];
      if (!first) continue;
      lines.push(`# TYPE ${metric} ${first.kind}`);
      for (const s of group) {
        if (s.kind === "counter" || s.kind === "gauge") {
          lines.push(`${metric}${renderLabels(s.labels)} ${s.value}`);
        } else {
          let cumulative = 0;
          for (let i = 0; i < this.buckets.length; i += 1) {
            cumulative = s.counts[i] ?? 0; // counts are already cumulative-eligible
            const le = String(this.buckets[i]);
            lines.push(`${metric}_bucket${this.withLe(s.labels, le)} ${cumulative}`);
          }
          lines.push(`${metric}_bucket${this.withLe(s.labels, "+Inf")} ${s.count}`);
          lines.push(`${metric}_sum${renderLabels(s.labels)} ${s.sum}`);
          lines.push(`${metric}_count${renderLabels(s.labels)} ${s.count}`);
        }
      }
    }
    return lines.length === 0 ? "" : `${lines.join("\n")}\n`;
  }

  /** Render labels with an additional `le` bucket-bound label appended. */
  private withLe(labels: Labels | undefined, le: string): string {
    return renderLabels({ ...(labels ?? {}), le });
  }
}

/**
 * A no-op sink for tests or when metrics are disabled. Implements the full
 * interface so injection sites never need to null-check.
 */
export class NullMetrics implements MetricsSink {
  inc(): void {}
  gauge(): void {}
  observe(): void {}
  render(): string {
    return "";
  }
}

/** Well-known metric names so producers and the health endpoint agree. */
export const METRICS = {
  slicesFilled: "cadence_slices_filled_total",
  slicesSkipped: "cadence_slices_skipped_total",
  slicesPaused: "cadence_slices_paused_total",
  breakerTrips: "cadence_breaker_trips_total",
  confirmationLatencyMs: "cadence_confirmation_latency_ms",
  llmTokens: "cadence_llm_tokens_total",
  venueHealthy: "cadence_venue_healthy",
  lastConfirmedSliceAgeMs: "cadence_last_confirmed_slice_age_ms",
} as const;
