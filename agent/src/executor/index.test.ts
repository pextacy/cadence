import { describe, expect, it } from "vitest";
import { Executor, type ExecutorDeps } from "./index.js";
import type { ConfirmationResult, ConfirmationService } from "../clients/confirm.js";
import type { Quote, RuntimeMandate, SliceProposal, VaultState } from "../types.js";

/**
 * Confirmation-gating tests for the executor. The executor must never advance
 * past an unconfirmed/reverted on-chain submission: the swap may only fire after
 * `execute_slice` is confirmed, and `record_fill` only after the swap deploy is
 * confirmed. Fakes record call order so we can assert the gates short-circuit.
 */

const mandate: RuntimeMandate = {
  sellAsset: "CSPR",
  buyAsset: "USDC",
  totalSell: 1_000n,
  endTimeMs: 10_000_000_000_000,
  maxSlippageBps: 100,
  priceFloor: 0n,
  priceCeiling: 0n,
  venueAllowlist: ["cspr.trade"],
  venueAddresses: ["hash-venue"],
  strategy: "TWAP",
};

const state: VaultState = {
  status: "Active",
  soldSoFar: 0n,
  boughtSoFar: 0n,
  sliceCount: 0,
  totalSell: 1_000n,
};

const proposal: SliceProposal = {
  sellAmount: 100n,
  notBeforeMs: 0,
  maxSlippageBps: 100,
  reason: "test slice",
};

const quote: Quote = {
  venue: "cspr.trade",
  venueAddress: "hash-venue",
  sellAmount: 100n,
  quotedOut: 200n,
};

const SUCCESS: ConfirmationResult = { status: "success", blockHash: "blk", costMotes: "0" };

interface Calls {
  executeSlice: number;
  executeSwap: number;
  recordFill: number;
  attest: number;
}

function makeDeps(confirm: ConfirmationService): { deps: ExecutorDeps; calls: Calls } {
  const calls: Calls = { executeSlice: 0, executeSwap: 0, recordFill: 0, attest: 0 };
  const vault = {
    async executeSlice() {
      calls.executeSlice += 1;
      return "slice-tx";
    },
    async recordFill() {
      calls.recordFill += 1;
      return "record-tx";
    },
    async attest() {
      calls.attest += 1;
      return "attest-tx";
    },
  };
  const market = {
    async getQuotes() {
      return [quote];
    },
    async executeSwap() {
      calls.executeSwap += 1;
      return { deployHash: "swap-deploy", boughtAmount: 200n };
    },
  };
  const deps = {
    vault,
    market,
    confirm,
    sellToken: "CSPR",
    buyToken: "USDC",
    proceedsRecipient: "account-hash-treasury",
  } as unknown as ExecutorDeps;
  return { deps, calls };
}

function constantConfirm(result: ConfirmationResult): ConfirmationService {
  return {
    async confirmTransaction() {
      return result;
    },
    async confirmDeploy() {
      return result;
    },
  };
}

describe("Executor confirmation gating", () => {
  it("fills the slice when both submissions confirm successfully", async () => {
    const { deps, calls } = makeDeps(constantConfirm(SUCCESS));
    const outcome = await new Executor(deps).executeOnce(mandate, state, proposal, Date.now());
    expect(outcome.status).toBe("filled");
    expect(calls).toEqual({ executeSlice: 1, executeSwap: 1, recordFill: 1, attest: 1 });
  });

  it("pauses without swapping when execute_slice reverts on-chain", async () => {
    const confirm: ConfirmationService = {
      async confirmTransaction() {
        return { status: "failure", errorMessage: "SpendCapExceeded" };
      },
      async confirmDeploy() {
        return SUCCESS;
      },
    };
    const { deps, calls } = makeDeps(confirm);
    const outcome = await new Executor(deps).executeOnce(mandate, state, proposal, Date.now());
    expect(outcome.status).toBe("paused");
    expect(calls.executeSlice).toBe(1);
    // The swap must never fire against an unconfirmed slice.
    expect(calls.executeSwap).toBe(0);
    expect(calls.recordFill).toBe(0);
  });

  it("pauses without swapping when execute_slice never confirms (timeout)", async () => {
    const confirm: ConfirmationService = {
      async confirmTransaction() {
        return { status: "timeout" };
      },
      async confirmDeploy() {
        return SUCCESS;
      },
    };
    const { deps, calls } = makeDeps(confirm);
    const outcome = await new Executor(deps).executeOnce(mandate, state, proposal, Date.now());
    expect(outcome.status).toBe("paused");
    expect(calls.executeSwap).toBe(0);
  });

  it("pauses without recording the fill when the swap deploy is not confirmed", async () => {
    const confirm: ConfirmationService = {
      async confirmTransaction() {
        return SUCCESS;
      },
      async confirmDeploy() {
        return { status: "failure", errorMessage: "swap reverted" };
      },
    };
    const { deps, calls } = makeDeps(confirm);
    const outcome = await new Executor(deps).executeOnce(mandate, state, proposal, Date.now());
    expect(outcome.status).toBe("paused");
    expect(calls.executeSwap).toBe(1);
    // record_fill must not trust an unconfirmed swap.
    expect(calls.recordFill).toBe(0);
    expect(calls.attest).toBe(0);
  });
});
