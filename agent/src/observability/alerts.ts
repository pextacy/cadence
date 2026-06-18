/**
 * AlertSink: out-of-band notification of operationally significant events —
 * breaker trips, confirmation timeouts, venue ejections, emergency halts,
 * shutdown. Distinct from MetricsSink (aggregate counters) and AuditLog
 * (immutable forensic trail): an alert is a *push* to a human/on-call channel.
 *
 * Two impls, both dependency-free:
 *  - ConsoleAlertSink routes through the structured `log()` helper (no
 *    console.log in app code) so alerts ride the same JSON-line stream.
 *  - WebhookAlertSink POSTs JSON to a configured URL (Slack/PagerDuty/etc.)
 *    using the global `fetch` (Node 22), with a bounded timeout. Delivery
 *    failures are swallowed-and-logged: alerting must never crash the loop.
 *
 * A FanOutAlertSink composes several sinks so one emit reaches all channels.
 */
import { log } from "../runtime.js";

/** Alert severity, ordered least → most urgent. */
export type AlertSeverity = "info" | "warning" | "critical";

export interface Alert {
  readonly severity: AlertSeverity;
  /** Stable machine event name, e.g. "breaker_open", "confirmation_timeout". */
  readonly event: string;
  /** Human-readable one-line summary. */
  readonly message: string;
  readonly trackId?: string;
  readonly detail?: Readonly<Record<string, unknown>>;
  readonly tsMs: number;
}

export interface AlertSink {
  /** Deliver an alert. Resolves once delivery is attempted; never throws. */
  notify(alert: Alert): Promise<void>;
}

/** Construct an Alert with `tsMs` defaulted to now. */
export function makeAlert(
  severity: AlertSeverity,
  event: string,
  message: string,
  opts: { trackId?: string; detail?: Readonly<Record<string, unknown>> } = {},
): Alert {
  return {
    severity,
    event,
    message,
    ...(opts.trackId ? { trackId: opts.trackId } : {}),
    ...(opts.detail ? { detail: opts.detail } : {}),
    tsMs: Date.now(),
  };
}

/**
 * Routes alerts through the structured `log()` helper. Always succeeds.
 * Default sink when no webhook is configured.
 */
export class ConsoleAlertSink implements AlertSink {
  async notify(alert: Alert): Promise<void> {
    log("alert", {
      severity: alert.severity,
      alertEvent: alert.event,
      message: alert.message,
      ...(alert.trackId ? { trackId: alert.trackId } : {}),
      ...(alert.detail ? { detail: alert.detail } : {}),
    });
  }
}

export interface WebhookAlertOptions {
  readonly url: string;
  /** Per-request timeout. Default 5s. */
  readonly timeoutMs?: number;
  /** Optional minimum severity to forward (others are dropped). Default "info". */
  readonly minSeverity?: AlertSeverity;
  /** Extra headers (e.g. an Authorization bearer token). */
  readonly headers?: Readonly<Record<string, string>>;
}

const SEVERITY_RANK: Record<AlertSeverity, number> = { info: 0, warning: 1, critical: 2 };

/**
 * POSTs an alert as JSON to a webhook (Slack-compatible `{ text, ... }` plus the
 * full structured alert). Honors a severity floor and a bounded timeout. A
 * non-2xx response or network/timeout error is logged via `log()` and
 * swallowed — alert delivery must never propagate into the trading loop.
 */
export class WebhookAlertSink implements AlertSink {
  private readonly url: string;
  private readonly timeoutMs: number;
  private readonly minRank: number;
  private readonly headers: Readonly<Record<string, string>>;

  constructor(opts: WebhookAlertOptions) {
    this.url = opts.url;
    this.timeoutMs = opts.timeoutMs ?? 5_000;
    this.minRank = SEVERITY_RANK[opts.minSeverity ?? "info"];
    this.headers = opts.headers ?? {};
  }

  async notify(alert: Alert): Promise<void> {
    if (SEVERITY_RANK[alert.severity] < this.minRank) return;
    const body = JSON.stringify({
      text: `[${alert.severity.toUpperCase()}] ${alert.event}: ${alert.message}`,
      severity: alert.severity,
      event: alert.event,
      message: alert.message,
      ...(alert.trackId ? { trackId: alert.trackId } : {}),
      ...(alert.detail ? { detail: alert.detail } : {}),
      tsMs: alert.tsMs,
    });
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const res = await fetch(this.url, {
        method: "POST",
        headers: { "content-type": "application/json", ...this.headers },
        body,
        signal: controller.signal,
      });
      if (!res.ok) {
        log("alert_delivery_failed", {
          alertEvent: alert.event,
          status: res.status,
          reason: `http_${res.status}`,
        });
      }
    } catch (err) {
      log("alert_delivery_failed", {
        alertEvent: alert.event,
        reason: err instanceof Error ? err.message : String(err),
      });
    } finally {
      clearTimeout(timer);
    }
  }
}

/**
 * Composes multiple sinks; an emit fans out to all of them concurrently. One
 * sink failing never blocks the others (each is already non-throwing, but we
 * wrap defensively with allSettled).
 */
export class FanOutAlertSink implements AlertSink {
  private readonly sinks: readonly AlertSink[];

  constructor(sinks: readonly AlertSink[]) {
    this.sinks = sinks;
  }

  async notify(alert: Alert): Promise<void> {
    await Promise.allSettled(this.sinks.map((s) => s.notify(alert)));
  }
}

/** A no-op sink for tests or when alerting is disabled. */
export class NullAlertSink implements AlertSink {
  async notify(): Promise<void> {}
}
