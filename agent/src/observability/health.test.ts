import { describe, it, expect, afterEach } from "vitest";
import { request } from "node:http";
import { HealthServer, evaluateReadiness, type HealthState } from "./health.js";
import { InProcessMetrics } from "./metrics.js";

const baseState: HealthState = {
  loopLive: true,
  ready: true,
  draining: false,
  breakerState: "closed",
  lastConfirmedSliceMs: 1_000,
};

describe("evaluateReadiness", () => {
  it("is ready under nominal state", () => {
    const r = evaluateReadiness(baseState, 1_500, 0);
    expect(r.ready).toBe(true);
    expect(r.reasons).toHaveLength(0);
  });

  it("fails when the loop is not live", () => {
    const r = evaluateReadiness({ ...baseState, loopLive: false }, 1_500, 0);
    expect(r.ready).toBe(false);
    expect(r.reasons).toContain("loop_not_live");
  });

  it("fails when draining or breaker open", () => {
    expect(evaluateReadiness({ ...baseState, draining: true }, 1_500, 0).reasons).toContain(
      "draining",
    );
    expect(
      evaluateReadiness({ ...baseState, breakerState: "open" }, 1_500, 0).reasons,
    ).toContain("breaker_open");
  });

  it("flags a stalled slice only when staleness checking is enabled", () => {
    const old: HealthState = { ...baseState, lastConfirmedSliceMs: 0 };
    expect(evaluateReadiness(old, 100_000, 0).ready).toBe(true); // disabled
    expect(evaluateReadiness(old, 100_000, 10_000).reasons).toContain("slice_stalled");
  });

  it("not ready before initialisation completes", () => {
    expect(evaluateReadiness({ ...baseState, ready: false }, 1_500, 0).reasons).toContain(
      "not_initialised",
    );
  });
});

function get(port: number, path: string): Promise<{ status: number; body: string }> {
  return new Promise((resolve, reject) => {
    const req = request({ host: "127.0.0.1", port, path, method: "GET" }, (res) => {
      let body = "";
      res.on("data", (c) => (body += c));
      res.on("end", () => resolve({ status: res.statusCode ?? 0, body }));
    });
    req.on("error", reject);
    req.end();
  });
}

describe("HealthServer over a real socket", () => {
  let server: HealthServer | undefined;
  afterEach(async () => {
    await server?.stop();
    server = undefined;
  });

  it("serves /healthz, /readyz and /metrics", async () => {
    const metrics = new InProcessMetrics();
    metrics.inc("cadence_slices_filled_total", 2);
    let state: HealthState = { ...baseState };
    server = new HealthServer({ port: 39_517, snapshot: () => state, metrics });
    await server.start();

    const health = await get(39_517, "/healthz");
    expect(health.status).toBe(200);
    expect(JSON.parse(health.body).status).toBe("ok");

    const ready = await get(39_517, "/readyz");
    expect(ready.status).toBe(200);
    expect(JSON.parse(ready.body).ready).toBe(true);

    const metricsRes = await get(39_517, "/metrics");
    expect(metricsRes.status).toBe(200);
    expect(metricsRes.body).toContain("cadence_slices_filled_total 2");

    state = { ...state, draining: true };
    const notReady = await get(39_517, "/readyz");
    expect(notReady.status).toBe(503);
    expect(JSON.parse(notReady.body).reasons).toContain("draining");

    const missing = await get(39_517, "/nothing");
    expect(missing.status).toBe(404);
  });
});
