import { expect, test } from "@playwright/test";

/**
 * CreateMandate is the highest-value flow: it signs a mandate entirely client-side
 * (no network), so these assertions exercise the real secp256k1 signing path.
 *
 * A well-known Hardhat/Anvil test private key is used — it is public, throwaway,
 * and deterministic, so the derived signer address is stable across runs.
 */
const TEST_PRIVATE_KEY =
  "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

const HEX_DIGEST = /^0x[0-9a-fA-F]{64}$/;
const HEX_ADDRESS = /^0x[0-9a-fA-F]{40}$/;

/** Fill the order form with a valid, self-consistent mandate (one venue + one address). */
async function fillValidForm(page: import("@playwright/test").Page): Promise<void> {
  await page.fill("#sell", "CSPR");
  await page.fill("#buy", "USDC");
  await page.fill("#amount", "2000000");
  await page.selectOption("#strategy", "TWAP");
  await page.fill("#slip", "1.00");
  await page.fill("#venue", "cspr.trade");
  await page.fill("#venueAddresses", "0x1111111111111111111111111111111111111111");
}

test.describe("CreateMandate", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/#/mandate");
    await expect(page.getByRole("heading", { name: "Authorise the whole execution once" })).toBeVisible();
  });

  test("signs a valid mandate and renders digest, signature and signer", async ({ page }) => {
    await fillValidForm(page);

    const signButton = page.getByRole("button", { name: "Sign mandate" });

    // Disabled until a valid signing key is present.
    await expect(signButton).toBeDisabled();

    await page.fill("#devkey", TEST_PRIVATE_KEY);
    await expect(signButton).toBeEnabled();

    await signButton.click();

    // Three .codeblock outputs render: digest, signature, recovered signer.
    const blocks = page.locator(".codeblock");
    await expect(blocks).toHaveCount(3);

    const digest = (await blocks.nth(0).innerText()).trim();
    const signature = (await blocks.nth(1).innerText()).trim();
    const signer = (await blocks.nth(2).innerText()).trim();

    expect(digest).toMatch(HEX_DIGEST);
    expect(signature.startsWith("0x")).toBe(true);
    expect(signature.length).toBeGreaterThan(2);
    expect(signer).toMatch(HEX_ADDRESS);

    // The download button only appears after a successful sign.
    await expect(page.getByRole("button", { name: "Download signed mandate" })).toBeVisible();
  });

  test("keeps Sign disabled for an invalid (too short) signing key", async ({ page }) => {
    await fillValidForm(page);

    const signButton = page.getByRole("button", { name: "Sign mandate" });
    await expect(signButton).toBeDisabled();

    // A clearly-too-short hex key never matches the 32-byte regex.
    await page.fill("#devkey", "0xdeadbeef");
    await expect(signButton).toBeDisabled();

    // No outputs are produced.
    await expect(page.locator(".codeblock")).toHaveCount(0);
  });

  test("shows a venue/address count-mismatch error", async ({ page }) => {
    await fillValidForm(page);

    // Two venues but only one address -> validation error in a .error div.
    await page.fill("#venue", "cspr.trade, dex.two");
    await page.fill("#venueAddresses", "0x1111111111111111111111111111111111111111");

    const venueError = page.locator(".error", { hasText: "Provide one address per venue" });
    await expect(venueError).toBeVisible();

    // With a mismatch the mandate is invalid, so signing stays disabled even with a valid key.
    await page.fill("#devkey", TEST_PRIVATE_KEY);
    await expect(page.getByRole("button", { name: "Sign mandate" })).toBeDisabled();
  });
});
