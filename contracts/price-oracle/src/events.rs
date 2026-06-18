//! Events emitted by the [`SignedPriceOracle`](crate::signed_oracle::SignedPriceOracle).

use odra::casper_types::U512;
use odra::prelude::*;

/// Emitted when a signed price is accepted and becomes the latest for its pair.
#[odra::event]
pub struct PriceUpdated {
    pub pair: String,
    pub price: U512,
    pub timestamp_ms: u64,
    pub round: u64,
}

/// Emitted at `init` (and on rotation) recording the authorized operator account.
#[odra::event]
pub struct OperatorSet {
    pub operator: Address,
    pub max_staleness_ms: u64,
}
