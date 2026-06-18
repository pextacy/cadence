/**
 * NonceManager — serializes signed vault submissions on the shared agent key
 * and assigns a monotonic client-side sequence used as an idempotency key in
 * the StateStore.
 *
 * Casper transactions are NOT account-nonce-sequenced like EVM, so this is not
 * an on-chain nonce: it is a client-side concurrency guard that (1) prevents two
 * portfolio tracks sharing the agent key from submitting overlapping txs, and
 * (2) hands each logical submission a stable sequence so a retried/crash-replayed
 * `execute_slice` is recognised as the SAME logical slice (dedup) rather than
 * re-sent — closing the "nonce too old"/double-spend class of bug on retries.
 *
 * The sequence is a high-water mark, not a counter of completed work: it only
 * ever increases. On boot, {@link NonceManager.restore} re-seats it from the
 * StateStore so replayed sequences are never reissued to new submissions.
 */
export interface NonceManager {
  /**
   * Serialize a signed submission for the agent key. The provided `fn` runs
   * with exclusive access to the key (no other `withSequence` body overlaps it)
   * and receives a monotonic client sequence to use as the StateStore
   * idempotency key. Resolves/rejects with whatever `fn` returns/throws; the
   * lock is always released, even on throw.
   */
  withSequence<T>(fn: (seq: number) => Promise<T>): Promise<T>;
  /** Current high-water sequence (for recovery / dedup). */
  current(): number;
  /** Restore the high-water mark on boot from the StateStore. */
  restore(seq: number): void;
}

/**
 * In-process {@link NonceManager}: a single-key serialization mutex plus a
 * monotonic sequence. All `withSequence` calls queue on one promise chain so at
 * most one submission holds the agent key at a time, regardless of how many
 * portfolio tracks call concurrently.
 *
 * Not mutation-of-shared-state in the domain sense — the sequence/lock are this
 * object's own private bookkeeping, never an input that leaks out.
 */
export class InProcessNonceManager implements NonceManager {
  /** Tail of the serialization queue; each acquire chains onto it. */
  private tail: Promise<void> = Promise.resolve();
  /** Monotonic high-water sequence; `next()` returns `seq + 1`. */
  private seq: number;

  /**
   * @param startSeq Initial high-water mark. Defaults to 0 so the first issued
   *   sequence is 1. Pass the StateStore's recovered high-water on boot, or call
   *   {@link restore} afterwards.
   */
  constructor(startSeq = 0) {
    if (!Number.isInteger(startSeq) || startSeq < 0) {
      throw new Error(`NonceManager startSeq must be a non-negative integer, got ${startSeq}`);
    }
    this.seq = startSeq;
  }

  current(): number {
    return this.seq;
  }

  restore(seq: number): void {
    if (!Number.isInteger(seq) || seq < 0) {
      throw new Error(`NonceManager.restore expects a non-negative integer, got ${seq}`);
    }
    // High-water mark only ever advances: a stale/lower restore is ignored so a
    // partial recovery can never reissue an already-consumed sequence.
    if (seq > this.seq) this.seq = seq;
  }

  async withSequence<T>(fn: (seq: number) => Promise<T>): Promise<T> {
    // Acquire the lock by chaining onto the current tail. `release` is resolved
    // once this critical section completes, letting the next waiter proceed.
    let release!: () => void;
    const next = new Promise<void>((resolve) => {
      release = resolve;
    });
    const previous = this.tail;
    this.tail = next;

    // Wait for all prior submissions to finish before taking the key.
    await previous;
    const seq = ++this.seq;
    try {
      return await fn(seq);
    } finally {
      release();
    }
  }
}
