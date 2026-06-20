import { expect, test } from "@playwright/test";
import { liveSequence } from "./fixtures/vault-events.js";

/**
 * LiveExecution renders entirely from the CSPR.cloud WebSocket. We stub the socket
 * with Playwright's `page.routeWebSocket`, matching the `/contract-events` path the
 * `useVaultStream` hook opens (`${streamingUrl}/contract-events?contract_hash=...`).
 *
 * On the client's subscribe frame we push the canned lifecycle, so the screen leaves
 * its "connecting / waiting" states and reconstructs real state.
 */
test.describe("LiveExecution (stubbed stream)", () => {
  test("shows status badge, metrics and a slice row from streamed events", async ({ page }) => {
    const events = liveSequence();

    // Match the contract-events channel regardless of host/query string.
    await page.routeWebSocket(/contract-events/, (ws) => {
      // The hook sends a subscribe frame on open; once we see it, the listener
      // is attached and it is safe to push the canned events.
      ws.onMessage(() => {
        for (const ev of events) {
          ws.send(JSON.stringify(ev));
        }
      });
    });

    await page.goto("/#/execution");

    // Status badge becomes "Active" after VaultFunded. The first .badge inside the
    // mandate card is the StatusBadge (SliceBadges live later in the feed table).
    const badge = page.locator("main .card .badge").first();
    await expect(badge).toBeVisible();
    await expect(badge).toContainText(/Active/i);

    // Headline metrics grid is present.
    const stats = page.locator(".stat-grid .stat");
    await expect(stats.first()).toBeVisible();
    await expect(stats).toHaveCount(4);

    // At least one slice row in the feed table.
    const sliceRows = page.locator("table.feed tbody tr");
    await expect(sliceRows.first()).toBeVisible();
    expect(await sliceRows.count()).toBeGreaterThanOrEqual(1);

    // The filled slice's venue is rendered.
    await expect(page.locator("table.feed tbody")).toContainText("cspr.trade");
  });
});
