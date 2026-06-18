import { describe, it, expect, vi, afterEach } from "vitest";
import {
  WebhookAlertSink,
  FanOutAlertSink,
  ConsoleAlertSink,
  NullAlertSink,
  makeAlert,
  type Alert,
  type AlertSink,
} from "./alerts.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const a = (sev: Alert["severity"], event = "breaker_open"): Alert =>
  makeAlert(sev, event, "the breaker tripped", { detail: { reason: "volatility" } });

describe("makeAlert", () => {
  it("stamps tsMs and carries optional fields", () => {
    const alert = makeAlert("warning", "venue_ejected", "cspr cooling", { trackId: "t1" });
    expect(alert.tsMs).toBeGreaterThan(0);
    expect(alert.trackId).toBe("t1");
    expect(alert.severity).toBe("warning");
  });
});

describe("ConsoleAlertSink", () => {
  it("never throws", async () => {
    const sink = new ConsoleAlertSink();
    await expect(sink.notify(a("info"))).resolves.toBeUndefined();
  });
});

describe("WebhookAlertSink", () => {
  it("POSTs JSON to the configured url", async () => {
    const fetchMock = vi.fn(async () => new Response("", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const sink = new WebhookAlertSink({ url: "https://hooks.example/x" });
    await sink.notify(a("critical"));
    expect(fetchMock).toHaveBeenCalledOnce();
    const [url, init] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("https://hooks.example/x");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body.event).toBe("breaker_open");
    expect(body.text).toContain("[CRITICAL]");
  });

  it("drops alerts below the configured minSeverity", async () => {
    const fetchMock = vi.fn(async () => new Response("", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const sink = new WebhookAlertSink({ url: "https://h/x", minSeverity: "critical" });
    await sink.notify(a("warning"));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("swallows a non-2xx response without throwing", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const sink = new WebhookAlertSink({ url: "https://h/x" });
    await expect(sink.notify(a("critical"))).resolves.toBeUndefined();
  });

  it("swallows a network error without throwing", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new Error("ECONNREFUSED");
      }),
    );
    const sink = new WebhookAlertSink({ url: "https://h/x" });
    await expect(sink.notify(a("critical"))).resolves.toBeUndefined();
  });
});

describe("FanOutAlertSink", () => {
  it("delivers to every sink even if one fails", async () => {
    const ok = { notify: vi.fn(async () => {}) } satisfies AlertSink;
    const boom: AlertSink = {
      notify: vi.fn(async () => {
        throw new Error("down");
      }),
    };
    const okTwo = { notify: vi.fn(async () => {}) } satisfies AlertSink;
    const fan = new FanOutAlertSink([ok, boom, okTwo]);
    await expect(fan.notify(a("warning"))).resolves.toBeUndefined();
    expect(ok.notify).toHaveBeenCalledOnce();
    expect(okTwo.notify).toHaveBeenCalledOnce();
  });
});

describe("NullAlertSink", () => {
  it("is a no-op", async () => {
    await expect(new NullAlertSink().notify(a("critical"))).resolves.toBeUndefined();
  });
});
