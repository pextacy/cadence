import { defineConfig, devices } from "@playwright/test";

/**
 * E2E config for the Cadence dashboard.
 *
 * The dev server is started by Playwright on a fixed port with deterministic
 * `VITE_*` env so the build reads a stable config. The streaming URL points at a
 * fake `ws://` endpoint that is never actually opened — the streaming specs stub
 * the WebSocket with Playwright's `page.routeWebSocket`, so no real CSPR.cloud
 * connection is made. CreateMandate needs no network at all (client-side signing).
 */
const PORT = Number(process.env.DASHBOARD_PORT ?? "5174") || 5174;
const BASE_URL = `http://localhost:${PORT}`;

// Deterministic, dummy values. The vault hash is a valid 0x-prefixed 64-hex
// string; the api key and baseline price are arbitrary but present so that
// `isStreamConfigured` returns true and the streaming screens leave the "idle"
// state and attempt a connection (which the specs intercept).
const TEST_ENV = {
  VITE_CSPR_CLOUD_STREAMING_URL: "ws://127.0.0.1:5174/stream",
  VITE_CSPR_CLOUD_API_KEY: "test-api-key",
  VITE_VAULT_CONTRACT_HASH:
    "0x" + "ab".repeat(32),
  VITE_NAIVE_BASELINE_PRICE: "1000000000",
};

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 1 : undefined,
  // In CI, emit the concise `line` reporter to the log AND an HTML report into
  // the default `playwright-report/` dir (matched by the CI upload step).
  // `open: "never"` keeps the runner from trying to launch a browser in CI.
  reporter: process.env.CI ? [["line"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: BASE_URL,
    headless: true,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "npm run dev",
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      DASHBOARD_PORT: String(PORT),
      // Signals the Vite config to bind the exact port (strictPort) so the dev
      // server fails fast on a clash instead of falling forward to another port
      // while Playwright polls the fixed `url` above (→ hang/timeout).
      E2E: "1",
      ...TEST_ENV,
    },
  },
});
