import { describe, expect, it } from "vitest";
import { assertDeployTargetAllowed } from "./network-guard.js";

describe("assertDeployTargetAllowed", () => {
  it("passes for testnet regardless of opt-in", () => {
    expect(() => assertDeployTargetAllowed("testnet", false)).not.toThrow();
    expect(() => assertDeployTargetAllowed("testnet", true)).not.toThrow();
  });

  it("passes for the testnet chain name (casper-test)", () => {
    expect(() => assertDeployTargetAllowed("casper-test", false)).not.toThrow();
  });

  it("throws for mainnet without opt-in", () => {
    expect(() => assertDeployTargetAllowed("mainnet", false)).toThrow(/ALLOW_MAINNET=true/);
  });

  it("passes for mainnet with opt-in", () => {
    expect(() => assertDeployTargetAllowed("mainnet", true)).not.toThrow();
  });

  it("treats the alias 'casper' as mainnet", () => {
    expect(() => assertDeployTargetAllowed("casper", false)).toThrow(/mainnet/i);
    expect(() => assertDeployTargetAllowed("casper", true)).not.toThrow();
  });

  it("is case-insensitive and ignores surrounding whitespace", () => {
    expect(() => assertDeployTargetAllowed("  MAINNET  ", false)).toThrow();
    expect(() => assertDeployTargetAllowed("Casper", false)).toThrow();
    expect(() => assertDeployTargetAllowed("  TESTNET  ", false)).not.toThrow();
  });
});
