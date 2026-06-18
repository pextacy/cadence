//! Events emitted by the [`FeeModule`](crate::fees::FeeModule).

use odra::casper_types::U512;
use odra::prelude::*;

/// Emitted when a protocol fee is accrued against a settled fill.
#[odra::event]
pub struct FeeAccrued {
    /// The asset symbol the fee is denominated in (e.g. the buy asset).
    pub asset: String,
    /// The notional amount the fee was charged on.
    pub amount: U512,
    /// The fee charged, in `asset` units (`amount * fee_bps / 10_000`).
    pub fee: U512,
    /// Fee rate applied, in basis points.
    pub fee_bps: u32,
    /// Account credited with the accrued fee (the fee collector).
    pub collector: Address,
}

/// Emitted when an account withdraws its accrued fee balance.
#[odra::event]
pub struct FeeWithdrawn {
    /// Account whose accrued balance was withdrawn.
    pub collector: Address,
    /// Recipient of the withdrawn balance.
    pub recipient: Address,
    /// Amount withdrawn.
    pub amount: U512,
}

/// Emitted when the protocol fee rate is changed.
#[odra::event]
pub struct FeeRateChanged {
    /// Fee rate before the change, in basis points.
    pub previous_bps: u32,
    /// Fee rate after the change, in basis points.
    pub new_bps: u32,
    /// Account that performed the change.
    pub sender: Address,
}
