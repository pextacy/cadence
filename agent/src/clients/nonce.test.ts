import { describe, it, expect } from "vitest";
import { InProcessNonceManager } from "./nonce.js";

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

describe("InProcessNonceManager", () => {
  it("issues strictly monotonic sequences starting at 1", async () => {
    const nm = new InProcessNonceManager();
    expect(nm.current()).toBe(0);
    const seqs: number[] = [];
    await nm.withSequence(async (s) => void seqs.push(s));
    await nm.withSequence(async (s) => void seqs.push(s));
    await nm.withSequence(async (s) => void seqs.push(s));
    expect(seqs).toEqual([1, 2, 3]);
    expect(nm.current()).toBe(3);
  });

  it("serializes concurrent submissions on the shared key (no overlap)", async () => {
    const nm = new InProcessNonceManager();
    const events: string[] = [];

    const make = (label: string) =>
      nm.withSequence(async (seq) => {
        events.push(`enter:${label}:${seq}`);
        await sleep(10);
        events.push(`exit:${label}:${seq}`);
        return seq;
      });

    // Fire all three "simultaneously"; they must run end-to-end one at a time.
    const [a, b, c] = await Promise.all([make("a"), make("b"), make("c")]);

    // Each enter is immediately followed by its own exit — never interleaved.
    expect(events).toEqual([
      "enter:a:1",
      "exit:a:1",
      "enter:b:2",
      "exit:b:2",
      "enter:c:3",
      "exit:c:3",
    ]);
    expect([a, b, c]).toEqual([1, 2, 3]);
  });

  it("releases the lock even when the body throws", async () => {
    const nm = new InProcessNonceManager();
    await expect(
      nm.withSequence(async () => {
        throw new Error("boom");
      }),
    ).rejects.toThrow("boom");
    // Lock must be free: a subsequent submission proceeds and advances the seq.
    const seq = await nm.withSequence(async (s) => s);
    expect(seq).toBe(2);
    expect(nm.current()).toBe(2);
  });

  it("restores the high-water mark, advancing only forward", () => {
    const nm = new InProcessNonceManager();
    nm.restore(50);
    expect(nm.current()).toBe(50);
    // Lower restore is ignored (no reissue of consumed sequences).
    nm.restore(10);
    expect(nm.current()).toBe(50);
    nm.restore(75);
    expect(nm.current()).toBe(75);
  });

  it("issues sequences above a restored high-water mark", async () => {
    const nm = new InProcessNonceManager(0);
    nm.restore(100);
    const seq = await nm.withSequence(async (s) => s);
    expect(seq).toBe(101);
  });

  it("accepts a constructor start sequence", async () => {
    const nm = new InProcessNonceManager(7);
    expect(nm.current()).toBe(7);
    const seq = await nm.withSequence(async (s) => s);
    expect(seq).toBe(8);
  });

  it("rejects invalid start sequences", () => {
    expect(() => new InProcessNonceManager(-1)).toThrow();
    expect(() => new InProcessNonceManager(1.5)).toThrow();
  });

  it("rejects invalid restore values", () => {
    const nm = new InProcessNonceManager();
    expect(() => nm.restore(-1)).toThrow();
    expect(() => nm.restore(2.5)).toThrow();
  });
});
