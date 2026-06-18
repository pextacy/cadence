import casper from "casper-js-sdk";
import type * as Casper from "casper-js-sdk";
import { networkPreset } from "@cadence/mandate";

// casper-js-sdk ships as CommonJS; its full API is on the default export. Named
// ESM imports are not detectable at runtime, so destructure values from default
// and take type-only names from the namespace import (erased at compile time).
const { CLValue, CLTypeUInt8, CLTypeString, CLTypeKey, HttpHandler, Key, KeyAlgorithm, PrivateKey, RpcClient } =
  casper;

/** Construct an RPC client for a node URL. */
export function makeRpc(nodeRpcUrl: string): Casper.RpcClient {
  return new RpcClient(new HttpHandler(nodeRpcUrl));
}

/** Load a secp256k1 private key from hex (with or without 0x). */
export function loadSecp256k1(hex: string): Casper.PrivateKey {
  return PrivateKey.fromHex(hex.replace(/^0x/, ""), KeyAlgorithm.SECP256K1);
}

/** Parse 0x-prefixed hex into a byte array. */
export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.replace(/^0x/, "");
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/**
 * Encode a byte blob as a `List<U8>` CLValue — the representation Odra uses for a
 * `Bytes` entrypoint argument.
 */
export function clBytesList(bytes: Uint8Array): Casper.CLValue {
  return CLValue.newCLList(
    CLTypeUInt8,
    Array.from(bytes, (b) => CLValue.newCLUint8(b)),
  );
}

/** Encode a list of strings as a `List<String>` CLValue. */
export function clStringList(items: string[]): Casper.CLValue {
  return CLValue.newCLList(
    CLTypeString,
    items.map((s) => CLValue.newCLString(s)),
  );
}

/**
 * Encode a list of Casper keys (account-hash-… / hash-…) as a `List<Key>` CLValue
 * — the representation Odra uses for a `Vec<Address>` entrypoint argument.
 */
export function clKeyList(items: string[]): Casper.CLValue {
  return CLValue.newCLList(
    CLTypeKey,
    items.map((k) => CLValue.newCLKey(Key.newKey(k))),
  );
}

/** Read a required environment variable or throw. */
export function requireEnv(name: string): string {
  const v = process.env[name];
  if (v === undefined || v === "") throw new Error(`Missing required environment variable: ${name}`);
  return v;
}

/**
 * Chain name for the selected network. `CASPER_NETWORK` (mainnet|testnet) chooses
 * the default; `CASPER_CHAIN_NAME` overrides it explicitly.
 */
export function networkChainName(): string {
  return process.env.CASPER_CHAIN_NAME ?? networkPreset(process.env.CASPER_NETWORK).chainName;
}

/**
 * JSON-RPC node URL for the selected network. `CASPER_NODE_RPC` overrides the
 * network preset's default node.
 */
export function networkNodeRpc(): string {
  return process.env.CASPER_NODE_RPC ?? networkPreset(process.env.CASPER_NETWORK).nodeRpcUrl;
}

/** Structured log line. */
export function log(event: string, detail: Record<string, unknown> = {}): void {
  console.log(JSON.stringify({ ts: new Date().toISOString(), event, ...detail }));
}
