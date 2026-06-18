//! Persistent state for the [`SettlementAdapter`]: the per-escrow custody record
//! and the module's storage layout.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

use super::errors::Error;
use super::events::{SettlementRecorded, SwapIntent};

/// One escrowed slice awaiting off-chain settlement.
#[odra::odra_type]
pub struct Escrow {
    pub vault: Address,
    pub sell_amount: U512,
    pub min_out: U512,
    pub recipient: Address,
    pub settled: bool,
}

/// Escrow + attested-settlement adapter for off-chain venues.
#[odra::module(events = [SwapIntent, SettlementRecorded], errors = Error)]
pub struct SettlementAdapter {
    /// Stable venue id this adapter settles for (e.g. `"cspr.trade"`).
    pub(super) venue_id: Var<String>,
    /// Settlement operator account; its key signs realised-fill attestations.
    pub(super) operator: Var<Address>,
    /// Monotonic escrow id counter.
    pub(super) next_escrow_id: Var<u64>,
    /// Per-escrow custody + terms record.
    pub(super) escrows: Mapping<u64, Escrow>,
    /// Spent attestation nonces, keyed by `(operator, nonce)` for replay protection.
    pub(super) used_attestations: Mapping<(Address, Bytes), bool>,
}
