//! The two settlement lifecycle events: [`SwapIntent`] (escrow booked) and
//! [`SettlementRecorded`] (attested fill proven).

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

/// Emitted when the vault escrows a slice for off-chain settlement. The agent and
/// settlement operator watch for this to know a swap is owed and which slice/escrow
/// it corresponds to.
#[odra::event]
pub struct SwapIntent {
    pub escrow_id: u64,
    pub vault: Address,
    pub sell_asset: String,
    pub buy_asset: String,
    pub sell_amount: U512,
    pub min_out: U512,
    pub recipient: Address,
}

/// Emitted after a settlement operator's attestation verifies on-chain, proving the
/// realised buy-asset amount for an escrowed slice.
#[odra::event]
pub struct SettlementRecorded {
    pub escrow_id: u64,
    pub bought_amount: U512,
    pub settlement_ref: Bytes,
    pub nonce: Bytes,
}
