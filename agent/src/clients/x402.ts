import {
  hashTypedData,
  recoverTypedDataSigner,
  TransferAuthorizationTypes,
  CASPER_DOMAIN_TYPES,
  buildDomain,
  toHex,
  fromHex,
} from "@casper-ecosystem/casper-eip-712";
import { secp256k1 } from "@noble/curves/secp256k1";
import type { PaymentProof } from "../types.js";

/** A single payment option from a 402 response, per the x402 protocol. */
export interface PaymentRequirements {
  scheme: string;
  network: string;
  payTo: string;
  amount: string;
  asset: string;
  maxTimeoutSeconds: number;
  extra?: { name: string; version: string };
}

/** Body of a 402 Payment Required response. */
export interface Payment402Body {
  x402Version: number;
  accepts: PaymentRequirements[];
  error?: string;
}

/** The EIP-712 transfer authorization carried in the payment payload. */
export interface TransferAuthorization {
  from: string;
  to: string;
  value: string;
  valid_after: number;
  valid_before: number;
  nonce: string;
}

/** The signed payment payload replayed in the `PAYMENT-SIGNATURE` header. */
export interface PaymentPayload {
  x402Version: number;
  resource: { url: string };
  accepted: PaymentRequirements;
  payload: {
    signature: string;
    publicKey: string;
    authorization: TransferAuthorization;
  };
}

export interface BuildPaymentOptions {
  /** Resource being paid for. */
  resourceUrl: string;
  /** Payer Casper account hash, 0x-prefixed 32-byte hex. */
  from: string;
  /** Replay-protection nonce, 0x-prefixed 32-byte hex. */
  nonce: string;
  /** Current unix time in seconds. */
  nowSec: number;
  /** Payer secp256k1 private key, hex. */
  privateKeyHex: string;
}

const RECENT_SKEW_SEC = 60;

/** Pick the Casper payment requirement matching the given network. */
export function selectCasperRequirement(
  body: Payment402Body,
  network: string,
): PaymentRequirements {
  const match = body.accepts.find((r) => r.network === network);
  if (!match) {
    throw new Error(`No x402 payment option for network ${network}`);
  }
  return match;
}

function authorizationDomain(req: PaymentRequirements) {
  const name = req.extra?.name ?? "Cep18x402";
  const version = req.extra?.version ?? "1";
  // The verifying contract is the payment asset (CEP-18 token package).
  return buildDomain(name, version, req.network, req.asset);
}

function authMessage(auth: TransferAuthorization): Record<string, unknown> {
  return {
    from: auth.from,
    to: auth.to,
    value: BigInt(auth.value),
    valid_after: auth.valid_after,
    valid_before: auth.valid_before,
    nonce: auth.nonce,
  };
}

/**
 * Build and sign an x402 payment payload for a Casper payment requirement. Pure
 * and deterministic given the options, so it can be unit-tested without a server.
 */
export function buildPaymentPayload(
  req: PaymentRequirements,
  opts: BuildPaymentOptions,
): PaymentPayload {
  const authorization: TransferAuthorization = {
    from: opts.from,
    to: req.payTo,
    value: req.amount,
    valid_after: opts.nowSec - RECENT_SKEW_SEC,
    valid_before: opts.nowSec + req.maxTimeoutSeconds,
    nonce: opts.nonce,
  };
  const domain = authorizationDomain(req);
  const digest = hashTypedData(
    domain,
    TransferAuthorizationTypes,
    "TransferAuthorization",
    authMessage(authorization),
    { domainTypes: CASPER_DOMAIN_TYPES },
  );
  const priv = fromHex(opts.privateKeyHex);
  const sig = secp256k1.sign(digest, priv);
  const full = new Uint8Array(65);
  full.set(sig.toCompactRawBytes(), 0);
  full[64] = sig.recovery + 27;
  const publicKey = toHex(secp256k1.getPublicKey(priv, false));
  return {
    x402Version: 2,
    resource: { url: opts.resourceUrl },
    accepted: req,
    payload: { signature: toHex(full), publicKey, authorization },
  };
}

/** Verify a payment payload's signature recovers a signer (integrity self-check). */
export function recoverPaymentSigner(payload: PaymentPayload): string {
  const domain = authorizationDomain(payload.accepted);
  const recovered = recoverTypedDataSigner(
    domain,
    TransferAuthorizationTypes,
    "TransferAuthorization",
    authMessage(payload.payload.authorization),
    fromHex(payload.payload.signature),
    { domainTypes: CASPER_DOMAIN_TYPES },
  );
  return toHex(recovered);
}

/** Base64-encode a payment payload for the `PAYMENT-SIGNATURE` header. */
export function encodePaymentHeader(payload: PaymentPayload): string {
  return Buffer.from(JSON.stringify(payload), "utf8").toString("base64");
}

function randomNonce(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return toHex(b);
}

export interface X402Result<T> {
  data: T;
  proof: PaymentProof;
}

/**
 * Fetch a premium resource behind an x402 paywall: GET, and on `402` build and
 * sign a payment, then replay the request with the `PAYMENT-SIGNATURE` header.
 * Returns the data and a payment proof for the audit trail.
 */
export async function fetchWithX402<T>(opts: {
  resourceUrl: string;
  network: string;
  from: string;
  privateKeyHex: string;
  init?: RequestInit;
}): Promise<X402Result<T>> {
  const first = await fetch(opts.resourceUrl, opts.init);
  if (first.status !== 402) {
    if (!first.ok) throw new Error(`x402 resource error ${first.status}`);
    throw new Error("x402 resource did not require payment; no proof to record");
  }
  const body = (await first.json()) as Payment402Body;
  const req = selectCasperRequirement(body, opts.network);
  const nonce = randomNonce();
  const payload = buildPaymentPayload(req, {
    resourceUrl: opts.resourceUrl,
    from: opts.from,
    nonce,
    nowSec: Math.floor(Date.now() / 1000),
    privateKeyHex: opts.privateKeyHex,
  });
  const header = encodePaymentHeader(payload);
  const paid = await fetch(opts.resourceUrl, {
    ...opts.init,
    headers: { ...(opts.init?.headers ?? {}), "PAYMENT-SIGNATURE": header },
  });
  if (!paid.ok) throw new Error(`x402 paid request failed ${paid.status}`);
  const data = (await paid.json()) as T;
  return {
    data,
    proof: {
      resource: opts.resourceUrl,
      network: req.network,
      payTo: req.payTo,
      amount: req.amount,
      asset: req.asset,
      signature: payload.payload.signature,
      nonce,
    },
  };
}
