import { describe, expect, it } from "vitest";
import { submissionDelayMs } from "./runtime.js";

describe("submissionDelayMs", () => {
  const now = 1_000_000;
  const deadline = now + 60_000;

  it("is zero when the slice is already due", () => {
    expect(submissionDelayMs(now, now, deadline)).toBe(0);
  });

  it("is zero when the scheduled time is in the past", () => {
    expect(submissionDelayMs(now - 5_000, now, deadline)).toBe(0);
  });

  it("waits exactly until a future scheduled time within the window", () => {
    expect(submissionDelayMs(now + 10_000, now, deadline)).toBe(10_000);
  });

  it("caps the wait at the deadline so it never blocks past the window", () => {
    // Scheduled well beyond the deadline → wait only until the deadline.
    expect(submissionDelayMs(deadline + 30_000, now, deadline)).toBe(60_000);
  });

  it("is never negative when already past the deadline", () => {
    expect(submissionDelayMs(now + 10_000, deadline + 1, deadline)).toBe(0);
  });

  it("treats a non-finite scheduled time as due now (never blocks)", () => {
    expect(submissionDelayMs(Number.NaN, now, deadline)).toBe(0);
    expect(submissionDelayMs(Number.POSITIVE_INFINITY, now, deadline)).toBe(0);
  });
});
