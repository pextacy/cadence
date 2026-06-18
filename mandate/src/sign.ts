import {
  hashTypedData,
  recoverTypedDataSigner,
  verifySignature,
  CASPER_DOMAIN_TYPES,
  toHex,
  fromHex,
  keccak256,
  type EIP712Domain,
} from "@casper-ecosystem/casper-eip-712";
import { secp256k1 } from "@noble/curves/secp256k1";
import {
  MANDATE_PRIMARY_TYPE,
  MANDATE_TYPES,
  toTypedMessage,
  type Mandate,
} from "./schema.js";

/** Recovery-id offset used by EIP-712 / Ethereum signatures (v ∈ {27, 28}). */
const V_OFFSET = 27;

export interface SignedMandate {
  mandate: Mandate;
  /** 0x-prefixed EIP-712 domain-bound digest (32 bytes). */
  digest: string;
  /** 0x-prefixed 65-byte secp256k1 signature (r ‖ s ‖ v). */
  signature: string;
  /** 0x-prefixed 20-byte recovered signer address. */
  signer: string;
}

/** Compute the EIP-712 digest for a mandate under the given domain. */
export function mandateDigest(mandate: Mandate, domain: EIP712Domain): Uint8Array {
  return hashTypedData(domain, MANDATE_TYPES, MANDATE_PRIMARY_TYPE, toTypedMessage(mandate), {
    domainTypes: CASPER_DOMAIN_TYPES,
  });
}

/** Derive the 0x-prefixed 20-byte signer address for a secp256k1 private key. */
export function addressFromPrivateKey(privateKeyHex: string): string {
  const priv = fromHex(privateKeyHex);
  const pub = secp256k1.getPublicKey(priv, false); // uncompressed, 65 bytes (0x04 prefix)
  const addr = keccak256(pub.slice(1)).slice(12); // last 20 bytes of keccak(pubkey[1:])
  return toHex(addr);
}

/**
 * Sign a mandate with a secp256k1 private key, producing the EIP-712 digest and a
 * 65-byte signature whose recovered address matches the key. The mandate's
 * `treasury` field is set to the derived signer address.
 */
export function signMandate(
  mandate: Omit<Mandate, "treasury"> & { treasury?: string },
  domain: EIP712Domain,
  privateKeyHex: string,
): SignedMandate {
  const signer = addressFromPrivateKey(privateKeyHex);
  const bound: Mandate = { ...mandate, treasury: signer } as Mandate;
  const digest = mandateDigest(bound, domain);
  const priv = fromHex(privateKeyHex);
  const sig = secp256k1.sign(digest, priv);
  const compact = sig.toCompactRawBytes(); // 64 bytes: r ‖ s
  const full = new Uint8Array(65);
  full.set(compact, 0);
  full[64] = sig.recovery + V_OFFSET;
  return {
    mandate: bound,
    digest: toHex(digest),
    signature: toHex(full),
    signer,
  };
}

export interface VerifyResult {
  valid: boolean;
  /** 0x-prefixed 20-byte recovered signer address (lowercase). */
  signer: string;
}

/**
 * Verify a mandate signature. Recovers the signer from the typed data and checks
 * it equals the mandate's `treasury` field. Returns the recovered address either
 * way so callers can surface a mismatch.
 */
export function verifyMandate(
  mandate: Mandate,
  domain: EIP712Domain,
  signatureHex: string,
): VerifyResult {
  const signature = fromHex(signatureHex);
  const recovered = recoverTypedDataSigner(
    domain,
    MANDATE_TYPES,
    MANDATE_PRIMARY_TYPE,
    toTypedMessage(mandate),
    signature,
    { domainTypes: CASPER_DOMAIN_TYPES },
  );
  const signer = toHex(recovered);
  const valid = signer.toLowerCase() === mandate.treasury.toLowerCase();
  return { valid, signer };
}

/** Verify a signature directly against the precomputed digest. */
export function verifyDigest(
  digestHex: string,
  signatureHex: string,
  expectedAddress: string,
): boolean {
  return verifySignature(fromHex(digestHex), fromHex(signatureHex), expectedAddress);
}
