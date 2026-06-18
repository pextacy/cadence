/**
 * Tiny HTTP health/readiness endpoint built on `node:http` (no framework, no
 * new dep). Exposes:
 *   - GET /healthz  liveness: 200 while the process is up.
 *   - GET /readyz   readiness: 200 when the loop is live and not stalled;
 *                   503 when draining, stalled, or the breaker is open.
 *   - GET /metrics  Prometheus text exposition from the MetricsSink.
 *
 * The server reads a `HealthState` snapshot provided by the loop via a
 * callback, so it never holds mutable loop state itself — the loop pushes the
 * latest snapshot at top-of-tick. Readiness reasons are returned in the body so
 * an operator/orchestrator can see *why* a probe failed.
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { MetricsSink } from "./metrics.js";
import { log } from "../runtime.js";

/** Snapshot of operational health the loop publishes each tick. */
export interface HealthState {
  /** True once the loop has started and is actively ticking. */
  readonly loopLive: boolean;
  /** True once boot-time recovery/reconciliation has completed. */
  readonly ready: boolean;
  /** True when a shutdown signal has begun draining. */
  readonly draining: boolean;
  /** Epoch ms of the last confirmed slice (or last successful tick). */
  readonly lastConfirmedSliceMs?: number;
  /** Circuit-breaker state. "open" means readiness should fail. */
  readonly breakerState: "closed" | "open";
  /** Per-venue health summary for the readiness body. */
  readonly venueHealth?: Readonly<Record<string, "up" | "cooling">>;
}

export interface HealthServerOptions {
  readonly port: number;
  /** Bind host. Default "0.0.0.0" so it is reachable inside a container. */
  readonly host?: string;
  /**
   * Max age (ms) of the last confirmed slice before readiness reports stalled.
   * Set 0 to disable the staleness check. Default 0 (disabled).
   */
  readonly maxSliceAgeMs?: number;
  /** Returns the current health snapshot. Called per request. */
  readonly snapshot: () => HealthState;
  /** Metrics registry rendered on /metrics. */
  readonly metrics: MetricsSink;
}

/** Result of evaluating readiness against a snapshot — pure and testable. */
export interface ReadinessResult {
  readonly ready: boolean;
  readonly reasons: readonly string[];
}

/**
 * Pure readiness evaluation. Extracted from the HTTP layer so it can be unit
 * tested without a socket. Not-ready when: loop not live, boot not ready,
 * draining, breaker open, or the last confirmed slice is older than
 * `maxSliceAgeMs` (when enabled).
 */
export function evaluateReadiness(
  state: HealthState,
  nowMs: number,
  maxSliceAgeMs: number,
): ReadinessResult {
  const reasons: string[] = [];
  if (!state.loopLive) reasons.push("loop_not_live");
  if (!state.ready) reasons.push("not_initialised");
  if (state.draining) reasons.push("draining");
  if (state.breakerState === "open") reasons.push("breaker_open");
  if (
    maxSliceAgeMs > 0 &&
    state.lastConfirmedSliceMs !== undefined &&
    nowMs - state.lastConfirmedSliceMs > maxSliceAgeMs
  ) {
    reasons.push("slice_stalled");
  }
  return { ready: reasons.length === 0, reasons };
}

function send(res: ServerResponse, status: number, contentType: string, body: string): void {
  res.writeHead(status, { "content-type": contentType });
  res.end(body);
}

/**
 * HTTP health server. `start()` resolves once listening; `stop()` resolves once
 * closed (call from the ShutdownController cleanup chain). Pure routing logic is
 * delegated to `evaluateReadiness`.
 */
export class HealthServer {
  private server: Server | undefined;
  private readonly opts: Required<Pick<HealthServerOptions, "host" | "maxSliceAgeMs">> &
    HealthServerOptions;

  constructor(opts: HealthServerOptions) {
    this.opts = { host: opts.host ?? "0.0.0.0", maxSliceAgeMs: opts.maxSliceAgeMs ?? 0, ...opts };
  }

  /** The request handler, exposed for unit tests without a live socket. */
  handle = (req: IncomingMessage, res: ServerResponse): void => {
    const url = (req.url ?? "/").split("?")[0];
    if (req.method !== "GET") {
      send(res, 405, "text/plain", "method not allowed");
      return;
    }
    if (url === "/healthz") {
      const live = this.opts.snapshot().loopLive;
      send(res, live ? 200 : 503, "application/json", JSON.stringify({ status: live ? "ok" : "down" }));
      return;
    }
    if (url === "/readyz") {
      const state = this.opts.snapshot();
      const result = evaluateReadiness(state, Date.now(), this.opts.maxSliceAgeMs);
      send(
        res,
        result.ready ? 200 : 503,
        "application/json",
        JSON.stringify({
          ready: result.ready,
          reasons: result.reasons,
          breaker: state.breakerState,
          ...(state.venueHealth ? { venues: state.venueHealth } : {}),
          ...(state.lastConfirmedSliceMs !== undefined
            ? { lastConfirmedSliceMs: state.lastConfirmedSliceMs }
            : {}),
        }),
      );
      return;
    }
    if (url === "/metrics") {
      send(res, 200, "text/plain; version=0.0.4", this.opts.metrics.render());
      return;
    }
    send(res, 404, "text/plain", "not found");
  };

  /** Begin listening. Resolves once bound; rejects on listen error. */
  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      const server = createServer(this.handle);
      server.on("error", (err) => reject(err));
      server.listen(this.opts.port, this.opts.host, () => {
        this.server = server;
        log("health_server_listening", { port: this.opts.port, host: this.opts.host });
        resolve();
      });
    });
  }

  /** Stop the server. Resolves once closed (idempotent). */
  stop(): Promise<void> {
    return new Promise((resolve) => {
      if (!this.server) {
        resolve();
        return;
      }
      this.server.close(() => {
        this.server = undefined;
        log("health_server_stopped", {});
        resolve();
      });
    });
  }
}
