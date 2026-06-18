import { describe, it, expect } from "vitest";
import { InProcessMetrics, NullMetrics, DEFAULT_BUCKETS_MS } from "./metrics.js";

describe("InProcessMetrics counters", () => {
  it("accumulates and renders a counter with labels", () => {
    const m = new InProcessMetrics();
    m.inc("cadence_slices_filled_total", 1, { venue: "cspr" });
    m.inc("cadence_slices_filled_total", 2, { venue: "cspr" });
    const out = m.render();
    expect(out).toContain("# TYPE cadence_slices_filled_total counter");
    expect(out).toContain('cadence_slices_filled_total{venue="cspr"} 3');
  });

  it("keeps distinct label sets as separate series", () => {
    const m = new InProcessMetrics();
    m.inc("c", 1, { v: "a" });
    m.inc("c", 5, { v: "b" });
    const out = m.render();
    expect(out).toContain('c{v="a"} 1');
    expect(out).toContain('c{v="b"} 5');
  });

  it("ignores invalid metric names and negative counter values", () => {
    const m = new InProcessMetrics();
    m.inc("123-bad", 1);
    m.inc("ok_total", -4);
    expect(m.render()).toBe("");
  });

  it("escapes label values per the exposition format", () => {
    const m = new InProcessMetrics();
    m.inc("c", 1, { reason: 'a"b\\c' });
    expect(m.render()).toContain('reason="a\\"b\\\\c"');
  });
});

describe("InProcessMetrics gauges", () => {
  it("overwrites with the latest absolute value and allows decreases", () => {
    const m = new InProcessMetrics();
    m.gauge("cadence_venue_healthy", 3);
    m.gauge("cadence_venue_healthy", 1);
    const out = m.render();
    expect(out).toContain("# TYPE cadence_venue_healthy gauge");
    expect(out).toContain("cadence_venue_healthy 1");
  });
});

describe("InProcessMetrics histograms", () => {
  it("renders cumulative buckets plus sum and count", () => {
    const m = new InProcessMetrics([100, 1000]);
    m.observe("lat_ms", 50);
    m.observe("lat_ms", 500);
    m.observe("lat_ms", 5000);
    const out = m.render();
    expect(out).toContain("# TYPE lat_ms histogram");
    expect(out).toContain('lat_ms_bucket{le="100"} 1');
    expect(out).toContain('lat_ms_bucket{le="1000"} 2');
    expect(out).toContain('lat_ms_bucket{le="+Inf"} 3');
    expect(out).toContain("lat_ms_sum 5550");
    expect(out).toContain("lat_ms_count 3");
  });

  it("ships sane default buckets sorted ascending", () => {
    const sorted = [...DEFAULT_BUCKETS_MS].sort((a, b) => a - b);
    expect(DEFAULT_BUCKETS_MS).toEqual(sorted);
  });
});

describe("type-clash safety", () => {
  it("ignores an observe on a name already used as a counter", () => {
    const m = new InProcessMetrics();
    m.inc("x", 1);
    m.observe("x", 10);
    const out = m.render();
    expect(out).toContain("# TYPE x counter");
    expect(out).not.toContain("x_bucket");
  });
});

describe("NullMetrics", () => {
  it("is a no-op that renders empty", () => {
    const m = new NullMetrics();
    m.inc("a");
    m.gauge("b", 1);
    m.observe("c", 2);
    expect(m.render()).toBe("");
  });
});
