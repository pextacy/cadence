import { describe, it, expect } from "vitest";
import { parsePortfolioManifest } from "./manifest.js";

const valid = {
  mandates: [
    { signedMandatePath: "./mandate.a.signed.json", vaultContractHash: "hash-a" },
    { signedMandatePath: "./mandate.b.signed.json", vaultContractHash: "hash-b" },
  ],
};

describe("parsePortfolioManifest", () => {
  it("parses a valid manifest", () => {
    const m = parsePortfolioManifest(valid);
    expect(m.mandates).toHaveLength(2);
    expect(m.mandates[0]?.vaultContractHash).toBe("hash-a");
  });

  it("rejects an empty mandate list", () => {
    expect(() => parsePortfolioManifest({ mandates: [] })).toThrow();
  });

  it("rejects a missing field", () => {
    expect(() => parsePortfolioManifest({ mandates: [{ signedMandatePath: "x" }] })).toThrow();
  });

  it("accepts an optional per-entry treasury account hash", () => {
    const m = parsePortfolioManifest({
      mandates: [
        { signedMandatePath: "./a.json", vaultContractHash: "hash-a", treasuryAccountHash: "account-hash-aa" },
      ],
    });
    expect(m.mandates[0]?.treasuryAccountHash).toBe("account-hash-aa");
  });

  it("rejects duplicate vault contract hashes (custody isolation)", () => {
    const dup = {
      mandates: [
        { signedMandatePath: "./a.json", vaultContractHash: "same" },
        { signedMandatePath: "./b.json", vaultContractHash: "same" },
      ],
    };
    expect(() => parsePortfolioManifest(dup)).toThrow(/vault/i);
  });
});
