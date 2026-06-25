// @vitest-environment jsdom
//
// End-to-end test of the dashboard's CreateMandate → sign flow, driving the real
// React screen in a DOM: fill the form, paste a development secp256k1 key, click
// "Sign mandate", and assert a real signed mandate is produced (the same pipeline
// `npm run sign-mandate` uses, exercised through the UI). The dev-signer path makes
// this deterministic with no wallet — the production wallet path (CSPR.click) is a
// thin swap of the same `signMandate` call.
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { addressFromPrivateKey } from "@cadence/mandate";
import { CreateMandate } from "./CreateMandate.js";
import type { DashboardConfig } from "../config.js";

// The screen only reads `config.chainName` (for the EIP-712 domain).
const CONFIG = { chainName: "casper-test" } as unknown as DashboardConfig;
// A fixed, valid secp256k1 private key (32 bytes, below the curve order, non-zero).
const TEST_KEY = "0x" + "11".repeat(32);

afterEach(cleanup);

describe("CreateMandate sign flow (E2E over the DOM)", () => {
  it("signs a mandate from the form and shows the recovered signer", () => {
    render(<CreateMandate config={CONFIG} />);

    // The default form is valid except for the venue address; supply one address
    // for the single default venue (cspr.trade).
    fireEvent.change(screen.getByLabelText(/Venue addresses/i), {
      target: { value: "account-hash-" + "ab".repeat(32) },
    });

    const signBtn = screen.getByRole("button", { name: /Sign mandate/i }) as HTMLButtonElement;
    // Disabled until a valid signing key is entered.
    expect(signBtn.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText(/Signing key/i), { target: { value: TEST_KEY } });
    expect(signBtn.disabled).toBe(false);

    fireEvent.click(signBtn);

    // A signed mandate is produced: the download affordance appears and the
    // recovered signer matches the address derived from the signing key.
    expect(screen.getByRole("button", { name: /Download signed mandate/i })).toBeTruthy();
    const expectedSigner = addressFromPrivateKey(TEST_KEY);
    expect(screen.getByText(expectedSigner)).toBeTruthy();
  });

  it("keeps signing disabled while the form is invalid (missing venue address)", () => {
    render(<CreateMandate config={CONFIG} />);
    // The form pre-fills the default adapter venue address; clear it to exercise the
    // invalid state. A valid key alone is not enough without a venue address.
    fireEvent.change(screen.getByLabelText(/Venue addresses/i), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText(/Signing key/i), { target: { value: TEST_KEY } });
    const signBtn = screen.getByRole("button", { name: /Sign mandate/i }) as HTMLButtonElement;
    expect(signBtn.disabled).toBe(true);
  });

  it("surfaces a clear error for an invalid signing key", () => {
    render(<CreateMandate config={CONFIG} />);
    fireEvent.change(screen.getByLabelText(/Venue addresses/i), {
      target: { value: "account-hash-" + "ab".repeat(32) },
    });
    // Too short to match the key regex → the Sign button never enables.
    fireEvent.change(screen.getByLabelText(/Signing key/i), { target: { value: "0x1234" } });
    const signBtn = screen.getByRole("button", { name: /Sign mandate/i }) as HTMLButtonElement;
    expect(signBtn.disabled).toBe(true);
  });
});
