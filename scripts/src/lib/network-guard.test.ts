import { describe, expect, it } from "vitest";
import { NETWORK_PRESETS } from "@cadence/mandate";
import { assertDeployTargetAllowed, type DeployTarget } from "./network-guard.js";

const TESTNET = NETWORK_PRESETS.testnet;
const MAINNET = NETWORK_PRESETS.mainnet;

/** A fully-testnet effective target (the safe default). */
function testnetTarget(overrides: Partial<DeployTarget> = {}): DeployTarget {
  return {
    network: "testnet",
    chainName: TESTNET.chainName, // "casper-test"
    nodeRpc: TESTNET.nodeRpcUrl, // https://node.testnet.cspr.cloud/rpc
    ...overrides,
  };
}

describe("assertDeployTargetAllowed", () => {
  it("passes for a fully-testnet target regardless of opt-in", () => {
    expect(() => assertDeployTargetAllowed(testnetTarget(), false)).not.toThrow();
    expect(() => assertDeployTargetAllowed(testnetTarget(), true)).not.toThrow();
  });

  it("passes for the testnet chain name (casper-test) without opt-in", () => {
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "casper-test" }), false),
    ).not.toThrow();
  });

  it("throws for mainnet (by network) without opt-in", () => {
    expect(() =>
      assertDeployTargetAllowed(
        testnetTarget({ network: "mainnet" }),
        false,
      ),
    ).toThrow(/ALLOW_MAINNET=true/);
  });

  it("passes for mainnet (by network) with opt-in", () => {
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "mainnet" }), true),
    ).not.toThrow();
  });

  it("treats the alias 'casper' (as network selector) as mainnet", () => {
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "casper" }), false),
    ).toThrow(/mainnet/i);
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "casper" }), true),
    ).not.toThrow();
  });

  it("is case-insensitive and ignores surrounding whitespace on network", () => {
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "  MAINNET  " }), false),
    ).toThrow();
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "Casper" }), false),
    ).toThrow();
    expect(() =>
      assertDeployTargetAllowed(testnetTarget({ network: "  TESTNET  " }), false),
    ).not.toThrow();
  });

  // --- Hole-closing cases: these FAIL against the old single-variable logic that
  // looked only at `network`. The effective chain name / node RPC point at mainnet
  // while `network` stays on the testnet default. ---

  it("throws for mainnet via CASPER_CHAIN_NAME (network on testnet default) without opt-in", () => {
    const target = testnetTarget({ chainName: MAINNET.chainName }); // network "testnet", chain "casper"
    expect(() => assertDeployTargetAllowed(target, false)).toThrow(/ALLOW_MAINNET=true/);
  });

  it("passes the mainnet-chain-name target WITH opt-in", () => {
    const target = testnetTarget({ chainName: MAINNET.chainName });
    expect(() => assertDeployTargetAllowed(target, true)).not.toThrow();
  });

  it("throws for mainnet via CASPER_NODE_RPC host (network on testnet default) without opt-in", () => {
    const target = testnetTarget({ nodeRpc: MAINNET.nodeRpcUrl }); // network "testnet", mainnet RPC host
    expect(() => assertDeployTargetAllowed(target, false)).toThrow(/ALLOW_MAINNET=true/);
  });

  it("passes the mainnet-node-rpc target WITH opt-in", () => {
    const target = testnetTarget({ nodeRpc: MAINNET.nodeRpcUrl });
    expect(() => assertDeployTargetAllowed(target, true)).not.toThrow();
  });

  it("detects a mainnet node RPC host even without a URL scheme", () => {
    const noScheme = MAINNET.nodeRpcUrl.replace(/^https?:\/\//, ""); // node.mainnet.cspr.cloud/rpc
    const target = testnetTarget({ nodeRpc: noScheme });
    expect(() => assertDeployTargetAllowed(target, false)).toThrow(/ALLOW_MAINNET=true/);
  });

  it("throws for the combined mainnet chain name + node RPC without opt-in", () => {
    const target = testnetTarget({
      chainName: MAINNET.chainName,
      nodeRpc: MAINNET.nodeRpcUrl,
    });
    expect(() => assertDeployTargetAllowed(target, false)).toThrow(/ALLOW_MAINNET=true/);
  });

  it("passes the combined mainnet target WITH opt-in", () => {
    const target = testnetTarget({
      chainName: MAINNET.chainName,
      nodeRpc: MAINNET.nodeRpcUrl,
    });
    expect(() => assertDeployTargetAllowed(target, true)).not.toThrow();
  });
});
