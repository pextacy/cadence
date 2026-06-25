import casper from "casper-js-sdk";
import type * as Casper from "casper-js-sdk";
import {
  buildMandatePreimage,
  bytesToHexPure,
  networkPreset,
  parseCasperAddress,
  resolveNetwork,
} from "@cadence/mandate";

// casper-js-sdk ships as CommonJS; its full API is on the default export. Named
// ESM imports are not detectable at runtime, so destructure values from default
// and take type-only names from the namespace import (erased at compile time).
const { Args, CLValue, CLTypeUInt8, CLTypeString, CLTypeKey, HttpHandler, Key, KeyAlgorithm, PrivateKey, RpcClient } =
  casper;

/**
 * Build the runtime args an Odra contract's WASM `call()` requires on a fresh
 * install. Odra's own deployer injects these `odra_cfg_*` flags (see
 * odra-core host.rs `try_deploy_with_cfg`); without them the install reverts with
 * a framework user-error. `packageHashKeyName` is the account named key the new
 * package hash is registered under (read it back to resolve the contract address).
 * Merge the contract's own constructor args via `ctorArgs`.
 */
export function odraInstallArgs(
  packageHashKeyName: string,
  ctorArgs: Record<string, Casper.CLValue> = {},
): Casper.Args {
  return Args.fromMap({
    odra_cfg_is_upgradable: CLValue.newCLValueBool(false),
    odra_cfg_is_upgrade: CLValue.newCLValueBool(false),
    odra_cfg_allow_key_override: CLValue.newCLValueBool(false),
    odra_cfg_package_hash_key_name: CLValue.newCLString(packageHashKeyName),
    ...ctorArgs,
  });
}

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

/**
 * Encode exactly 32 bytes as a fixed `ByteArray(32)` CLValue — the representation
 * Odra uses for a `[u8; 32]` entrypoint argument (e.g. the registry's
 * `mandate_hash`). Distinct from `clBytesList` (a length-prefixed `List<U8>`, used
 * for Odra `Bytes`); a fixed array is NOT interchangeable with a byte list.
 */
export function clByteArray32(bytes: Uint8Array): Casper.CLValue {
  if (bytes.length !== 32) {
    throw new Error(`clByteArray32 expects exactly 32 bytes, got ${bytes.length}`);
  }
  return CLValue.newCLByteArray(bytes);
}

/** The treasury's Casper-native authorization over a mandate's frozen preimage. */
export interface CasperMandateAuth {
  /** 0x-prefixed Casper signature (1 algorithm byte + 64-byte secp256k1) over the preimage. */
  casperSignature: string;
  /** Treasury Casper public key hex — passed to `init` as `treasury_public_key`. */
  treasuryPublicKey: string;
  /** 0x-prefixed canonical preimage bytes, for audit/debugging. */
  preimage: string;
}

/** The mandate fields the Casper-native preimage binds (mirrors the vault's `init` args). */
export interface CasperMandateAuthParams {
  /** Agent identity that will call `execute_slice` ("account-hash-…"). */
  agentAccountHash: string;
  sellAsset: string;
  buyAsset: string;
  totalSell: bigint;
  /** Deadline in milliseconds (the vault stores ms; the EIP-712 mandate carries seconds). */
  endTimeMs: bigint;
  maxSlippageBps: number;
  priceFloor: bigint;
  priceCeiling: bigint;
  venues: string[];
  /** One Casper address per venue (account-hash-… / hash-…), index-aligned with `venues`. */
  venueAddresses: string[];
  /** Mandate nonce hex (with or without 0x). */
  nonceHex: string;
}

/**
 * Produce the treasury's Casper-native mandate authorization. The vault's `init`
 * verifies `casper_signature` against the canonical preimage on-chain, so this is
 * what actually authorizes the limits (the EIP-712 digest is the human artifact).
 *
 * `treasury` in the preimage is the signer's Casper account — the same key MUST
 * send the install transaction, since `init` checks `treasury_public_key` hashes to
 * its caller. The signature is `signAndAddAlgorithmBytes` (Casper-tagged, secp256k1
 * over SHA-256 of the preimage) so on-chain `verify_signature` accepts it.
 */
export function signCasperMandate(
  treasuryKey: Casper.PrivateKey,
  p: CasperMandateAuthParams,
): CasperMandateAuth {
  const treasuryHash = hexToBytes(treasuryKey.publicKey.accountHash().toHex());
  const preimage = buildMandatePreimage({
    agent: parseCasperAddress(p.agentAccountHash),
    treasury: { tag: 0, hash: treasuryHash },
    sellAsset: p.sellAsset,
    buyAsset: p.buyAsset,
    totalSell: p.totalSell,
    endTimeMs: p.endTimeMs,
    maxSlippageBps: p.maxSlippageBps,
    priceFloor: p.priceFloor,
    priceCeiling: p.priceCeiling,
    venues: p.venues,
    venueAddresses: p.venueAddresses.map(parseCasperAddress),
    nonce: hexToBytes(p.nonceHex),
  });
  const sig = treasuryKey.signAndAddAlgorithmBytes(preimage);
  return {
    casperSignature: `0x${bytesToHexPure(sig)}`,
    treasuryPublicKey: treasuryKey.publicKey.toHex(),
    preimage: `0x${bytesToHexPure(preimage)}`,
  };
}

/** Serialized empty Casper RuntimeArgs (u32 count = 0). Used as the inner-call args
 * for payable entrypoints that take no parameters (vault `fund`, adapter
 * `seed_reserve`). */
export const EMPTY_RUNTIME_ARGS = new Uint8Array([0, 0, 0, 0]);

/**
 * Runtime args for Odra's `proxy_caller_with_return` session shim — the only way to
 * attach native CSPR to a `#[odra(payable)]` entrypoint (a plain stored-contract
 * call cannot carry value). Mirrors odra-casper-rpc-client's
 * `deploy_entrypoint_call_with_proxy`: the proxy creates a cargo purse, moves
 * `amount` from the caller's main purse into it, then invokes `entry_point` on the
 * package with the inner `args`. CLTypes match what the proxy deserializes:
 * package_hash = ByteArray(32), entry_point = String, args = List<U8> (the inner
 * RuntimeArgs `ToBytes`), attached_value/amount = U512.
 */
export function proxyCallArgs(
  packageHash: string,
  entryPoint: string,
  innerArgs: Uint8Array,
  amountMotes: bigint,
): Casper.Args {
  const pkg = hexToBytes(packageHash.replace(/^(hash-|contract-package-)/, ""));
  if (pkg.length !== 32) {
    throw new Error(`package hash must be 32 bytes, got ${pkg.length} from "${packageHash}"`);
  }
  return Args.fromMap({
    package_hash: CLValue.newCLByteArray(pkg),
    entry_point: CLValue.newCLString(entryPoint),
    args: clBytesList(innerArgs),
    attached_value: CLValue.newCLUInt512(amountMotes.toString()),
    amount: CLValue.newCLUInt512(amountMotes.toString()),
  });
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
  // A blank override (the .env default) must fall back to the preset — `||` so an
  // empty string counts as unset, not as a valid empty chain name.
  return process.env.CASPER_CHAIN_NAME?.trim() || networkPreset(process.env.CASPER_NETWORK).chainName;
}

/**
 * JSON-RPC node URL for the selected network. `CASPER_NODE_RPC` overrides the
 * network preset's default node.
 */
export function networkNodeRpc(): string {
  // Blank override → preset (see networkChainName). `||` treats "" as unset.
  return process.env.CASPER_NODE_RPC?.trim() || networkPreset(process.env.CASPER_NETWORK).nodeRpcUrl;
}

/**
 * The resolved network selector (`mainnet` | `testnet`) for this run, from
 * `CASPER_NETWORK` (defaults to testnet).
 */
export function selectedNetwork(): "mainnet" | "testnet" {
  return resolveNetwork(process.env.CASPER_NETWORK);
}

/**
 * Log a prominent banner naming the network this command will act on, so an
 * operator never deploys to or funds the wrong chain — `mainnet` spends real
 * CSPR. Reads the exact same resolution (`selectedNetwork`/`networkChainName`/
 * `networkNodeRpc`) the command itself uses, so the banner can never disagree
 * with where the transaction actually goes.
 */
export function logNetworkBanner(command: string): void {
  const network = selectedNetwork();
  log("network_target", {
    command,
    network,
    chainName: networkChainName(),
    nodeRpc: networkNodeRpc(),
    note:
      network === "mainnet"
        ? "MAINNET — this transaction spends real CSPR"
        : "testnet — safe to experiment",
  });
}

/** Structured log line. */
export function log(event: string, detail: Record<string, unknown> = {}): void {
  console.log(JSON.stringify({ ts: new Date().toISOString(), event, ...detail }));
}
