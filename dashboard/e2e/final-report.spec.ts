import { expect, test } from "@playwright/test";
import { settledSequence } from "./fixtures/vault-events.js";

/**
 * FinalReport renders only after the vault emits its Settled event. We stub the
 * CSPR.cloud WebSocket and push a full lifecycle ending in a completed Settled
 * event, then assert the outcome heading and settlement table.
 */
test.describe("FinalReport (stubbed stream)", () => {
  test("shows the completed outcome and settlement rows after a Settled event", async ({ page }) => {
    const events = settledSequence();

    await page.routeWebSocket(/contract-events/, (ws) => {
      ws.onMessage(() => {
        for (const ev of events) {
          ws.send(JSON.stringify(ev));
        }
      });
    });

    await page.goto("/#/report");

    // Outcome heading: "Order completed" for a completed settlement.
    await expect(page.getByRole("heading", { name: "Order completed" })).toBeVisible();

    // Settlement table rows are present and reflect the canned values.
    const settlementTable = page.locator("table.feed");
    await expect(settlementTable).toBeVisible();
    await expect(settlementTable).toContainText("Slices executed");
    await expect(settlementTable).toContainText("Returned to treasury");

    // Slice count of 2 from the Settled event.
    const sliceCountRow = page.locator("table.feed tr", { hasText: "Slices executed" });
    await expect(sliceCountRow.locator(".num")).toHaveText("2");
  });
});
