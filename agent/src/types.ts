import type { Mandate, Strategy } from "@cadence/mandate";

export type { Mandate, Strategy };

/** Vault lifecycle status, mirroring the on-chain `Status` enum. */
export type VaultStatus = "Funded" | "Active" | "Paused" | "Completed" | "Expired";

/**
 * The mandate limits in the runtime representation used by the executor and the
 * guardrail pre-checks. Amounts and prices are `bigint` in base units / fixed
 * point, exactly as the contract stores them. `endTimeMs` is milliseconds to
 * match the Casper block time.
 */
export interface RuntimeMandate {
  /** Symbol of the asset being sold (e.g. "CSPR"). */
  sellAsset: string;
  /** Symbol of the asset being bought (e.g. "USDC"). */
  buyAsset: string;
  totalSell: bigint;
  endTimeMs: number;
  maxSlippageBps: number;
  priceFloor: bigint; // 0n == unset
  priceCeiling: bigint; // 0n == unset
  venueAllowlist: readonly string[];
  strategy: Strategy;
}

/** A snapshot of vault progress read from chain. */
export interface VaultState {
  status: VaultStatus;
  soldSoFar: bigint;
  boughtSoFar: bigint;
  sliceCount: number;
  totalSell: bigint;
}

/** A quote returned by the CSPR.trade MCP for a prospective slice. */
export interface Quote {
  /** Venue identifier, e.g. "cspr.trade". */
  venue: string;
  /** On-chain address the sell asset is released to for the swap. */
  venueAddress: string;
  /** Sell amount this quote is for, in base units. */
  sellAmount: bigint;
  /** Expected output for `sellAmount`, in buy-asset base units. */
  quotedOut: bigint;
  /** Optional route identifier echoed back from the venue. */
  routeId?: string;
}

/** A market snapshot used by the planner. */
export interface MarketSnapshot {
  /** Mid price in fixed point (buy units per sell unit), if known. */
  midPrice: bigint;
  /** Annualised/representative volatility in basis points, if purchased. */
  volatilityBps?: number;
  /** Top-of-book depth on the sell side, in base units, if purchased. */
  depthSell?: bigint;
  /** Unix ms the snapshot was taken. */
  takenAtMs: number;
}

/** The planner's proposal for the next child order. Untrusted until validated. */
export interface SliceProposal {
  /** Size of the next child order in sell-asset base units. */
  sellAmount: bigint;
  /** Earliest time (unix ms) this slice should be submitted. */
  notBeforeMs: number;
  /** Per-slice slippage cap in bps (must be ≤ mandate cap). */
  maxSlippageBps: number;
  /** The planner's stated reason for this slice. */
  reason: string;
}

/** Proof of an x402 premium-data payment, recorded for the audit trail. */
export interface PaymentProof {
  resource: string;
  network: string;
  payTo: string;
  amount: string;
  asset: string;
  /** 0x-prefixed signature over the EIP-712 transfer authorization. */
  signature: string;
  nonce: string;
}
