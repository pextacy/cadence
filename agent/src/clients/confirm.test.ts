import { describe, it, expect, vi } from "vitest";
import type * as Casper from "casper-js-sdk";
import {
  confirmTransaction,
  RpcConfirmationService,
  type ConfirmationOutcome,
} from "./confirm.js";

/**
 * Build a fake ExecutionInfo. `errorMessage` empty/undefined => success.
 */
function execInfo(opts: {
  errorMessage?: string;
  blockHashHex?: string;
  blockHeight?: number;
  cost?: number;
}): Casper.ExecutionInfo {
  return {
    blockHash: { toHex: () => opts.blockHashHex ?? "deadbeef" } as unknown as Casper.Hash,
    blockHeight: opts.blockHeight ?? 42,
    executionResult: {
      errorMessage: opts.errorMessage,
      cost: opts.cost ?? 1234,
    } as unknown as Casper.ExecutionResult,
  } as Casper.ExecutionInfo;
}

/**
 * Minimal RpcClient stub whose `getTransactionByTransactionHash` returns the
 * next queued response per call (a value, or an Error to throw).
 */
function makeRpc(
  responses: ReadonlyArray<{ executionInfo?: Casper.ExecutionInfo } | Error>,
): Casper.RpcClient {
  let i = 0;
  const getTransactionByTransactionHash = vi.fn(async () => {
    const r = responses[Math.min(i, responses.length - 1)];
    i += 1;
    if (r instanceof Error) throw r;
    return r as Casper.InfoGetTransactionResult;
  });
  return {
    getTransactionByTransactionHash,
    getTransactionByDeployHash: getTransactionByTransactionHash,
    getDeploy: vi.fn(async () => {
      throw new Error("not found");
    }),
  } as unknown as Casper.RpcClient;
}

const HASH = "ab".repeat(32);

describe("confirmTransaction", () => {
  it("returns success when executionInfo has no errorMessage", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({ blockHashHex: "cafe", cost: 500 }) }]);
    const out = await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(out.status).toBe("success");
    expect(out.transactionHash).toBe(HASH);
    expect(out.blockHash).toBe("cafe");
    expect(out.cost).toBe("500");
    expect(out.attempts).toBe(1);
  });

  it("classifies an on-chain revert as failure", async () => {
    const rpc = makeRpc([
      { executionInfo: execInfo({ errorMessage: "User error: 17 (guardrail)" }) },
    ]);
    const out = await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(out.status).toBe("failure");
    expect(out.errorMessage).toBe("User error: 17 (guardrail)");
  });

  it("treats whitespace-only errorMessage as success", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({ errorMessage: "   " }) }]);
    const out = await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(out.status).toBe("success");
  });

  it("polls until the tx is included, treating not-found as pending", async () => {
    const rpc = makeRpc([
      { executionInfo: undefined },
      new Error("transaction not found"),
      { executionInfo: execInfo({}) },
    ]);
    const out = await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(out.status).toBe("success");
    expect(out.attempts).toBe(3);
  });

  it("returns timeout when the tx never lands within the budget", async () => {
    const rpc = makeRpc([{ executionInfo: undefined }]);
    const out = await confirmTransaction(rpc, HASH, { pollIntervalMs: 5, timeoutMs: 5 });
    expect(out.status).toBe("timeout");
    expect(out.attempts).toBe(1);
  });

  it("invokes onPoll once per attempt", async () => {
    const onPoll = vi.fn();
    const rpc = makeRpc([{ executionInfo: undefined }, { executionInfo: execInfo({}) }]);
    await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000, onPoll });
    expect(onPoll).toHaveBeenCalledTimes(2);
    expect(onPoll).toHaveBeenNthCalledWith(1, 1);
    expect(onPoll).toHaveBeenNthCalledWith(2, 2);
  });

  it("propagates non-not-found RPC errors", async () => {
    const rpc = makeRpc([new Error("connection refused")]);
    await expect(
      confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 }),
    ).rejects.toThrow(/connection refused/);
  });

  it("produces an immutable outcome (frozen-safe shape)", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({}) }]);
    const out: ConfirmationOutcome = await confirmTransaction(rpc, HASH, {
      pollIntervalMs: 1,
      timeoutMs: 1000,
    });
    // Re-running yields a fresh object, never a shared reference.
    const out2 = await confirmTransaction(rpc, HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(out).not.toBe(out2);
    expect(out).toEqual(out2);
  });
});

describe("RpcConfirmationService", () => {
  it("maps a success outcome to the boundary union with motes", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({ blockHashHex: "beef", cost: 99 }) }]);
    const svc = new RpcConfirmationService(rpc, { pollIntervalMs: 1, timeoutMs: 1000 });
    const res = await svc.confirmTransaction(HASH);
    expect(res).toEqual({ status: "success", blockHash: "beef", costMotes: "99" });
  });

  it("maps a revert to a failure union", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({ errorMessage: "halted" }) }]);
    const svc = new RpcConfirmationService(rpc);
    const res = await svc.confirmTransaction(HASH, { pollIntervalMs: 1, timeoutMs: 1000 });
    expect(res).toEqual({ status: "failure", errorMessage: "halted" });
  });

  it("confirmDeploy resolves via the transaction-by-deploy-hash endpoint", async () => {
    const rpc = makeRpc([{ executionInfo: execInfo({ blockHashHex: "f00d", cost: 7 }) }]);
    const svc = new RpcConfirmationService(rpc, { pollIntervalMs: 1, timeoutMs: 1000 });
    const res = await svc.confirmDeploy("cd".repeat(16));
    expect(res).toEqual({ status: "success", blockHash: "f00d", costMotes: "7" });
  });

  it("confirmDeploy times out when the deploy never lands", async () => {
    const rpc = makeRpc([{ executionInfo: undefined }]);
    const svc = new RpcConfirmationService(rpc, { pollIntervalMs: 5, timeoutMs: 5 });
    const res = await svc.confirmDeploy("cd".repeat(16));
    expect(res).toEqual({ status: "timeout" });
  });

  it("per-call options override construction defaults", async () => {
    const onPoll = vi.fn();
    const rpc = makeRpc([{ executionInfo: execInfo({}) }]);
    const svc = new RpcConfirmationService(rpc, { pollIntervalMs: 9999, timeoutMs: 9999 });
    await svc.confirmTransaction(HASH, { pollIntervalMs: 1, timeoutMs: 1000, onPoll });
    expect(onPoll).toHaveBeenCalledTimes(1);
  });
});
