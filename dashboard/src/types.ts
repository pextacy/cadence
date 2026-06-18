/** Vault lifecycle status, mirroring the on-chain `Status` enum. */
export type VaultStatus = "Funded" | "Active" | "Paused" | "Completed" | "Expired";

/**
 * On-chain events emitted by the Execution Vault, as delivered over CSPR.cloud
 * streaming. These are the single source of truth the dashboard reconstructs
 * state from — there is no mock layer.
 */
export type VaultEvent =
  | { kind: "MandateInitialised"; treasury: string; agent: string; totalSell: string; endTimeMs: number; maxSlippageBps: number }
  | { kind: "VaultFunded"; amount: string; balance: string }
  | { kind: "SliceExecuted"; sliceId: number; sellAmount: string; quotedOut: string; minOut: string; venue: string; soldSoFar: string; deployHash?: string; atMs?: number }
  | { kind: "FillRecorded"; sliceId: number; boughtAmount: string; swapDeployHash: string; boughtSoFar: string }
  | { kind: "DecisionAttested"; sliceId: number; reason: string }
  | { kind: "StatusChanged"; paused: boolean }
  | { kind: "Settled"; completed: boolean; soldSoFar: string; boughtSoFar: string; sliceCount: number; returnedToTreasury: string };

/** A per-slice view assembled from the SliceExecuted / FillRecorded / attest events. */
export interface SliceView {
  sliceId: number;
  sellAmount: bigint;
  quotedOut: bigint;
  minOut: bigint;
  venue: string;
  sliceDeployHash?: string;
  boughtAmount?: bigint;
  swapDeployHash?: string;
  reason?: string;
  status: "pending" | "filled" | "blocked";
  /** Observed time of the slice (from the streaming envelope), unix ms. */
  atMs?: number;
}

/** The reconstructed dashboard state. */
export interface DashboardState {
  status: VaultStatus | "Unknown";
  treasury?: string;
  agent?: string;
  totalSell: bigint;
  soldSoFar: bigint;
  boughtSoFar: bigint;
  endTimeMs?: number;
  maxSlippageBps?: number;
  slices: SliceView[];
  settled?: {
    completed: boolean;
    sliceCount: number;
    returnedToTreasury: bigint;
  };
}
