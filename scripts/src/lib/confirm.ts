import type * as Casper from "casper-js-sdk";

/**
 * Immutable result of polling a submitted Casper transaction to finality.
 *
 * - `success`: `executionInfo` is present (tx is in a finalized block) and the
 *   execution result carries no `errorMessage`.
 * - `failure`: `executionInfo` is present but `errorMessage` is set — an
 *   on-chain revert (e.g. a vault guardrail rejecting the deploy).
 * - `timeout`: the tx was not included within the polling budget.
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
  /** Per-attempt hook so callers can emit a structured log line without confirm.ts depending on a logger. */
  readonly onPoll?: (attempt: number) => void;
}

const DEFAULT_POLL_INTERVAL_MS = 5_000;
const DEFAULT_TIMEOUT_MS = 180_000;
/** Backoff cap: intervals grow until this ceiling so slow finality does not hammer the node. */
const MAX_POLL_INTERVAL_MS = 30_000;

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Core finality primitive. Loops calling
 * `rpc.getTransactionByTransactionHash(transactionHash)`; resolves once
 * `result.executionInfo` is defined (tx in a block), inspecting
 * `executionInfo.executionResult.errorMessage` to classify success/failure;
 * returns `status: "timeout"` after `timeoutMs`.
 *
 * Never throws on a missing/not-yet-included tx — a query error or absent
 * `executionInfo` is treated as "not yet finalized" and retried with bounded
 * exponential backoff (capped at {@link MAX_POLL_INTERVAL_MS}).
 */
export async function confirmTransaction(
  rpc: Casper.RpcClient,
  transactionHash: string,
  opts: ConfirmOptions = {},
): Promise<ConfirmationOutcome> {
  const baseInterval = opts.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const deadline = Date.now() + timeoutMs;

  let attempts = 0;
  let interval = baseInterval;

  while (Date.now() < deadline) {
    attempts += 1;
    opts.onPoll?.(attempts);

    const info = await safeGetTransaction(rpc, transactionHash);
    const executionInfo = info?.executionInfo;

    if (executionInfo !== undefined) {
      const result = executionInfo.executionResult;
      const errorMessage = result?.errorMessage;
      const blockHash = executionInfo.blockHash?.toHex();
      const blockHeight = executionInfo.blockHeight;
      const cost = result?.cost !== undefined ? String(result.cost) : undefined;

      // casper-js-sdk returns `null` (not undefined) for a successful tx's
      // errorMessage; a truthy check treats null/undefined/"" all as success and
      // only a non-empty string as a real on-chain revert.
      if (errorMessage) {
        return {
          transactionHash,
          status: "failure",
          blockHash,
          blockHeight,
          errorMessage,
          cost,
          attempts,
        };
      }

      return {
        transactionHash,
        status: "success",
        blockHash,
        blockHeight,
        cost,
        attempts,
      };
    }

    const remaining = deadline - Date.now();
    if (remaining <= 0) break;
    await sleep(Math.min(interval, remaining));
    interval = Math.min(interval * 2, MAX_POLL_INTERVAL_MS);
  }

  return { transactionHash, status: "timeout", attempts };
}

/**
 * Query a transaction, swallowing the "not found / not yet included" error the
 * node returns before a tx lands. Any error is treated as not-yet-finalized so
 * the caller keeps polling rather than aborting prematurely.
 */
async function safeGetTransaction(
  rpc: Casper.RpcClient,
  transactionHash: string,
): Promise<Casper.InfoGetTransactionResult | undefined> {
  try {
    return await rpc.getTransactionByTransactionHash(transactionHash);
  } catch {
    return undefined;
  }
}
