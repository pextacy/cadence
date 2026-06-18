import { describe, it, expect } from "vitest";
import { twapBaseline, extractJson } from "./index.js";

describe("twapBaseline", () => {
  it("splits remaining evenly across remaining slices", () => {
    expect(twapBaseline({ remaining: 1_000_000n, slicesRemaining: 10 })).toBe(100_000n);
  });
  it("returns the remainder when one slice is left", () => {
    expect(twapBaseline({ remaining: 123_456n, slicesRemaining: 1 })).toBe(123_456n);
  });
  it("guards against zero/negative slices", () => {
    expect(twapBaseline({ remaining: 500n, slicesRemaining: 0 })).toBe(500n);
  });
});

describe("extractJson", () => {
  it("parses a bare JSON object", () => {
    expect(extractJson('{"sellAmount":"100"}')).toEqual({ sellAmount: "100" });
  });
  it("parses JSON inside a code fence", () => {
    expect(extractJson('```json\n{"a":1}\n```')).toEqual({ a: 1 });
  });
  it("parses JSON with surrounding prose", () => {
    expect(extractJson('Here is the plan: {"a":2} done')).toEqual({ a: 2 });
  });
  it("throws when there is no object", () => {
    expect(() => extractJson("no json here")).toThrow();
  });
});
