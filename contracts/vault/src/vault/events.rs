//! Events emitted across the vault lifecycle.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

/// Emitted once when the mandate is stored.
#[odra::event]
pub struct MandateInitialised {
    pub treasury: Address,
    pub agent: Address,
    pub mandate_digest: Bytes,
    pub signature: Bytes,
    pub sell_asset: String,
    pub buy_asset: String,
    pub total_sell: U512,
    pub end_time_ms: u64,
    pub max_slippage_bps: u32,
}

/// Emitted by `init` AFTER a successful on-chain `verify_signature`, recording the
/// consumed mandate nonce and the blake2b hash of the canonical preimage that was
/// actually verified — a binding that was checked on-chain, not merely stored.
#[odra::event]
pub struct MandateVerified {
    pub treasury: Address,
    pub mandate_nonce: Bytes,
    pub preimage_hash: Bytes,
}

/// Emitted when the vault receives the sell asset and becomes Active.
#[odra::event]
pub struct VaultFunded {
    pub amount: U512,
    pub balance: U512,
}

/// Emitted for every accepted slice. `sell_amount` of the sell asset has been
/// released to the allowlisted venue for the swap; `min_out` is the floor the
/// agent committed to off-chain.
#[odra::event]
pub struct SliceExecuted {
    pub slice_id: u32,
    pub sell_amount: U512,
    pub quoted_out: U512,
    pub min_out: U512,
    pub venue: String,
    pub sold_so_far: U512,
}

/// Emitted when realised swap proceeds are recorded against a slice, linking the
/// on-chain swap deploy for the audit trail.
#[odra::event]
pub struct FillRecorded {
    pub slice_id: u32,
    pub bought_amount: U512,
    pub swap_deploy_hash: String,
    pub bought_so_far: U512,
}

/// Emitted for every decision attestation.
#[odra::event]
pub struct DecisionAttested {
    pub slice_id: u32,
    pub reason: String,
}

/// Emitted on pause / resume.
#[odra::event]
pub struct StatusChanged {
    pub paused: bool,
}

/// Emitted when the treasury triggers the emergency drain. Records who triggered
/// it, how much was returned, and the progress at the time of the halt.
#[odra::event]
pub struct EmergencyWithdrawn {
    pub by: Address,
    pub returned_to_treasury: U512,
    pub sold_so_far: U512,
}

/// Emitted once on settlement with the final execution report.
#[odra::event]
pub struct Settled {
    pub completed: bool,
    pub sold_so_far: U512,
    pub bought_so_far: U512,
    pub slice_count: u32,
    pub returned_to_treasury: U512,
}
