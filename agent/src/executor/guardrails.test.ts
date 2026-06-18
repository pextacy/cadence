import { describe, it, expect } from "vitest";
import { validateSlice } from "./guardrails.js";
import type { Quote, RuntimeMandate, SliceProposal, VaultState } from "../types.js";

const NOW = 1_000_000;
const VENUE_ADDR = "account-hash-" + "00".repeat(32);

function mandate(over: Partial<RuntimeMandate> = {}): RuntimeMandate {
  return {
    sellAsset: "CSPR",
    buyAsset: "USDC",
    totalSell: 1_000_000n,
    endTimeMs: NOW + 1_000_000,
    maxSlippageBps: 100,
    priceFloor: 0n,
    priceCeiling: 0n,
    venueAllowlist: ["cspr.trade"],
    venueAddresses: [VENUE_ADDR],
    strategy: "TWAP",
    ...over,
  };
}

function state(over: Partial<VaultState> = {}): VaultState {
  return { status: "Active", soldSoFar: 0n, boughtSoFar: 0n, sliceCount: 0, totalSell: 1_000_000n, ...over };
}

function proposal(over: Partial<SliceProposal> = {}): SliceProposal {
  return { sellAmount: 100_000n, notBeforeMs: NOW, maxSlippageBps: 100, reason: "twap slice", ...over };
}

function quote(over: Partial<Quote> = {}): Quote {
  return { venue: "cspr.trade", venueAddress: VENUE_ADDR, sellAmount: 100_000n, quotedOut: 200_000n, ...over };
}

describe("validateSlice", () => {
  it("accepts a within-limits slice and returns min_out", () => {
    const r = validateSlice(mandate(), state(), proposal(), quote(), NOW);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.minOut).toBe(198_000n);
  });

  it("rejects when not active", () => {
    const r = validateSlice(mandate(), state({ status: "Paused" }), proposal(), quote(), NOW);
    expect(r).toMatchObject({ ok: false, code: "NotActive" });
  });

  it("rejects past the deadline", () => {
    const r = validateSlice(mandate(), state(), proposal(), quote(), NOW + 2_000_000);
    expect(r).toMatchObject({ ok: false, code: "DeadlinePassed" });
  });

  it("rejects over the spend cap", () => {
    const r = validateSlice(mandate(), state({ soldSoFar: 950_000n }), proposal({ sellAmount: 100_000n }), quote({ sellAmount: 100_000n }), NOW);
    expect(r).toMatchObject({ ok: false, code: "SpendCapExceeded" });
  });

  it("rejects an unallowlisted venue", () => {
    const r = validateSlice(mandate(), state(), proposal(), quote({ venue: "evil.dex" }), NOW);
    expect(r).toMatchObject({ ok: false, code: "VenueNotAllowed" });
  });

  it("rejects a quote whose venue address is not the mandate-bound one", () => {
    const r = validateSlice(
      mandate(),
      state(),
      proposal(),
      quote({ venueAddress: "account-hash-" + "99".repeat(32) }),
      NOW,
    );
    expect(r).toMatchObject({ ok: false, code: "VenueNotAllowed" });
  });

  it("rejects a price outside the band", () => {
    // band [1.5, 2.5]; quote priced at 3.0
    const r = validateSlice(
      mandate({ priceFloor: 1_500_000_000n, priceCeiling: 2_500_000_000n }),
      state(),
      proposal(),
      quote({ quotedOut: 300_000n }),
      NOW,
    );
    expect(r).toMatchObject({ ok: false, code: "PriceOutOfBand" });
  });

  it("rejects when quote sell amount disagrees with the proposal", () => {
    const r = validateSlice(mandate(), state(), proposal({ sellAmount: 100_000n }), quote({ sellAmount: 90_000n }), NOW);
    expect(r).toMatchObject({ ok: false });
  });
});
