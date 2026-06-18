import type * as Casper from "casper-js-sdk";

/**
 * Immutable result of polling a submitted Casper transaction to finality.
 *
 * - `status: 'success'` — `executionInfo` is present (tx included in a block) and
 *   the execution result carries no `errorMessage`.
 * - `status: 'failure'` — `executionInfo.executionResult.errorMessage` is set
 *   (an on-chain revert, e.g. a vault guardrail rejection).
 * - `status: 'timeout'` — the tx was not included within the polling budget.
 *
 * This is the agent-side mirror of `scripts/src/lib/confirm.ts`. It is duplicated
 * verbatim (identical signature) so deploy/fund/VaultClient all confirm
 * submissions the same way without a cross-workspace runtime dependency on
 * `@cadence/scripts`.
 */
export interface ConfirmationOutcome {
  readonly transactionHash: string;
  readonly status: "success" | "failure" | "timeout";
  readonly blockHash?: string;
  readonly blockHeight?: number;
  readonly errorMessage?: string;
  readonly cost?: string;
  readonly attempts: number;
}

/** Optional tuning for finality polling. */
export interface ConfirmOptions {
  readonly pollIntervalMs?: number;
  readonly timeoutMs?: number;
  /**
   * Optional per-attempt callback so callers can emit a structured log line
   * without `confirm.ts` depending on a specific logger.
   */
  readonly onPoll?: (attempt: number) => void;
}

/** Default poll cadence (ms) between finality checks. */
export const DEFAULT_POLL_INTERVAL_MS = 5_000;
/** Default overall finality budget (ms) before declaring a timeout. */
export const DEFAULT_TIMEOUT_MS = 180_000;

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/**
 * Extract the on-chain error message (if any) from an `InfoGetTransactionResult`.
 * An empty/whitespace-only message is treated as success.
 */
function errorMessageOf(executionInfo: Casper.ExecutionInfo): string | undefined {
  const raw = executionInfo.executionResult?.errorMessage;
  if (typeof raw !== "string") return undefined;
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function blockHashOf(executionInfo: Casper.ExecutionInfo): string | undefined {
  try {
    return executionInfo.blockHash?.toHex();
  } catch {
    return undefined;
  }
}

function costOf(executionInfo: Casper.ExecutionInfo): string | undefined {
  const cost = executionInfo.executionResult?.cost;
  return typeof cost === "number" ? cost.toString() : undefined;
}

/**
 * Core finality primitive. Loops calling `rpc.getTransactionByTransactionHash`;
 * resolves once `result.executionInfo` is defined (tx in a block), inspecting
 * `executionInfo.executionResult.errorMessage` to classify success/failure.
 * Returns `status: 'timeout'` after `timeoutMs`.
 *
 * Never throws on a missing/not-yet-included tx: a not-found / un-included
 * transaction simply produces another poll attempt until the budget elapses.
 */
export async function confirmTransaction(
  rpc: Casper.RpcClient,
  transactionHash: string,
  opts: ConfirmOptions = {},
): Promise<ConfirmationOutcome> {
  const pollIntervalMs = opts.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const deadline = Date.now() + timeoutMs;

  let attempts = 0;
  // Loop until the budget is exhausted; one final attempt is always allowed so a
  // zero/short timeout still performs at least one query.
  for (;;) {
    attempts += 1;
    opts.onPoll?.(attempts);

    const executionInfo = await queryExecutionInfo(rpc, transactionHash);
    if (executionInfo) {
      const errorMessage = errorMessageOf(executionInfo);
      const blockHash = blockHashOf(executionInfo);
      const cost = costOf(executionInfo);
      if (errorMessage) {
        return {
          transactionHash,
          status: "failure",
          attempts,
          errorMessage,
          ...(blockHash ? { blockHash } : {}),
          blockHeight: executionInfo.blockHeight,
          ...(cost ? { cost } : {}),
        };
      }
      return {
        transactionHash,
        status: "success",
        attempts,
        ...(blockHash ? { blockHash } : {}),
        blockHeight: executionInfo.blockHeight,
        ...(cost ? { cost } : {}),
      };
    }

    if (Date.now() + pollIntervalMs >= deadline) {
      return { transactionHash, status: "timeout", attempts };
    }
    await sleep(pollIntervalMs);
  }
}

/**
 * Query a transaction hash and return its `executionInfo` once the tx is in a
 * block, or `null` while it is not yet included. A not-found result (the node
 * has not seen the tx yet) is treated as not-yet-included rather than an error.
 */
async function queryExecutionInfo(
  rpc: Casper.RpcClient,
  transactionHash: string,
): Promise<Casper.ExecutionInfo | null> {
  try {
    const result = await rpc.getTransactionByTransactionHash(transactionHash);
    return result.executionInfo ?? null;
  } catch (err: unknown) {
    if (isNotFound(err)) return null;
    throw err;
  }
}

/**
 * Query a deploy hash to finality. cspr.trade's off-chain MCP returns a deploy
 * hash for the swap it submitted; we still confirm that deploy landed on-chain
 * so `record_fill` is never trusted on an unconfirmed swap. Prefers the newer
 * transaction-by-deploy-hash endpoint, falling back to the legacy deploy
 * endpoint when the node only exposes that.
 */
async function queryDeployExecutionInfo(
  rpc: Casper.RpcClient,
  deployHash: string,
): Promise<Casper.ExecutionInfo | null> {
  try {
    const result = await rpc.getTransactionByDeployHash(deployHash);
    return result.executionInfo ?? null;
  } catch (err: unknown) {
    if (isNotFound(err)) return null;
    // Fall back to the legacy getDeploy endpoint for nodes/historical deploys
    // not surfaced by the transaction-by-deploy-hash index.
    try {
      const legacy = await rpc.getDeploy(deployHash);
      return legacy.executionInfo ?? null;
    } catch (legacyErr: unknown) {
      if (isNotFound(legacyErr)) return null;
      throw legacyErr;
    }
  }
}

/** A not-yet-included tx surfaces as a "not found" RPC error; treat as pending. */
function isNotFound(err: unknown): boolean {
  const message = (err instanceof Error ? err.message : String(err)).toLowerCase();
  return (
    message.includes("not found") ||
    message.includes("no such") ||
    message.includes("does not exist") ||
    // Casper JSON-RPC "transaction not found" maps to code -32602 / -32000 in
    // some node versions; match defensively on those too.
    message.includes("-32000") ||
    message.includes("-32602")
  );
}

/**
 * Outcome shape used by the operational-layer `ConfirmationService` boundary
 * (a discriminated union). Adapted from a {@link ConfirmationOutcome}.
 */
export type ConfirmationResult =
  | { readonly status: "success"; readonly blockHash: string; readonly costMotes: string }
  | { readonly status: "failure"; readonly errorMessage: string }
  | { readonly status: "timeout" };

/**
 * Confirmation boundary the executor gates state advances on. Wraps the
 * {@link confirmTransaction} primitive with bounded backoff polling for both
 * vault entrypoint transactions and the off-chain cspr.trade swap deploy hash.
 */
export interface ConfirmationService {
  /** Poll a vault entrypoint transaction hash to finality. */
  confirmTransaction(txHash: string, opts?: ConfirmOptions): Promise<ConfirmationResult>;
  /** Poll a cspr.trade swap deploy hash to finality. */
  confirmDeploy(deployHash: string, opts?: ConfirmOptions): Promise<ConfirmationResult>;
}

/** Convert the rich {@link ConfirmationOutcome} into the boundary union. */
function toResult(outcome: ConfirmationOutcome): ConfirmationResult {
  switch (outcome.status) {
    case "success":
      return {
        status: "success",
        blockHash: outcome.blockHash ?? "",
        costMotes: outcome.cost ?? "0",
      };
    case "failure":
      return {
        status: "failure",
        errorMessage: outcome.errorMessage ?? "execution reverted",
      };
    case "timeout":
      return { status: "timeout" };
  }
}

/**
 * Default {@link ConfirmationService} implementation backed by a Casper
 * `RpcClient`. Pure transport wrapper: it owns no state and never mutates its
 * inputs, so it is safe to share across portfolio tracks. Per-call options
 * override the construction-time defaults.
 */
export class RpcConfirmationService implements ConfirmationService {
  private readonly rpc: Casper.RpcClient;
  private readonly defaults: Required<Omit<ConfirmOptions, "onPoll">>;

  constructor(rpc: Casper.RpcClient, defaults: ConfirmOptions = {}) {
    this.rpc = rpc;
    this.defaults = {
      pollIntervalMs: defaults.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
      timeoutMs: defaults.timeoutMs ?? DEFAULT_TIMEOUT_MS,
    };
  }

  async confirmTransaction(txHash: string, opts: ConfirmOptions = {}): Promise<ConfirmationResult> {
    const outcome = await confirmTransaction(this.rpc, txHash, this.merge(opts));
    return toResult(outcome);
  }

  async confirmDeploy(deployHash: string, opts: ConfirmOptions = {}): Promise<ConfirmationResult> {
    const merged = this.merge(opts);
    const pollIntervalMs = merged.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
    const timeoutMs = merged.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    const deadline = Date.now() + timeoutMs;

    let attempts = 0;
    for (;;) {
      attempts += 1;
      merged.onPoll?.(attempts);

      const executionInfo = await queryDeployExecutionInfo(this.rpc, deployHash);
      if (executionInfo) {
        const errorMessage = errorMessageOf(executionInfo);
        if (errorMessage) return { status: "failure", errorMessage };
        return {
          status: "success",
          blockHash: blockHashOf(executionInfo) ?? "",
          costMotes: costOf(executionInfo) ?? "0",
        };
      }

      if (Date.now() + pollIntervalMs >= deadline) return { status: "timeout" };
      await sleep(pollIntervalMs);
    }
  }

  /** Merge per-call options over construction defaults without mutating either. */
  private merge(opts: ConfirmOptions): ConfirmOptions {
    return {
      pollIntervalMs: opts.pollIntervalMs ?? this.defaults.pollIntervalMs,
      timeoutMs: opts.timeoutMs ?? this.defaults.timeoutMs,
      ...(opts.onPoll ? { onPoll: opts.onPoll } : {}),
    };
  }
}
