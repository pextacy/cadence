import type { RuntimeMandate, VaultState } from "../types.js";

/**
 * One mandate under portfolio management. Each track has its own Execution Vault
 * (one vault per mandate keeps custody isolation trivial — the portfolio layer
 * lives entirely in the agent and never commingles funds across mandates).
 */
export interface MandateTrack {
  /** Stable identifier — the vault contract hash the mandate is bound to. */
  readonly id: string;
  readonly mandate: RuntimeMandate;
  /** The track's current vault state (the contract remains the authority). */
  readonly state: VaultState;
}
