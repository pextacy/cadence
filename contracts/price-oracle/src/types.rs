//! Shared price types and the cross-contract oracle read interface.
//!
//! [`PriceData`] is the fixed-point price record stored by the signed oracle and
//! returned by every read. [`OracleAdapter`] is the single cross-contract read
//! interface: it is declared `#[odra::external_contract]` so a consumer (the
//! execution vault, or the [`OracleAggregator`](crate::aggregator)) can build an
//! `OracleAdapterContractRef::new(env, oracle_addr)` from a resolved `Address` and
//! call `latest_price` without depending on the concrete oracle type.

use odra::casper_types::U512;
use odra::prelude::*;

/// Fixed-point scale for prices: a `price` of `1 * PRICE_SCALE` means 1.0 buy per
/// sell unit. Keep in sync with the vault's `PRICE_SCALE` so band checks line up.
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// A latest accepted price for a pair, fixed-point at [`PRICE_SCALE`].
///
/// Mirrors the [`OracleAdapter`] interface so an external contract reference
/// deserializes it identically across the signed oracle and the aggregator.
#[odra::odra_type]
pub struct PriceData {
    /// Price in fixed-point base units (`actual_price * PRICE_SCALE`).
    pub price: U512,
    /// Casper block time (milliseconds) the operator stamped on the round.
    pub timestamp_ms: u64,
    /// Strictly-monotonic per-pair sequence number.
    pub round: u64,
}

/// The price read interface consumers call cross-contract.
///
/// Implemented by [`SignedPriceOracle`](crate::signed_oracle::SignedPriceOracle).
/// The aggregator resolves each configured source as an `OracleAdapterContractRef`.
///
/// Two reads with different failure modes:
///   - [`latest_price`](OracleAdapter::latest_price) is the strict consumer read;
///     it MUST revert if the pair is unset or the freshest price is stale. The
///     vault uses this directly.
///   - [`get_price`](OracleAdapter::get_price) is the non-reverting raw read used
///     by the aggregator, which applies its *own* staleness gate so one stale
///     source aborts neither the cross-call nor the quorum (it is simply dropped).
#[odra::external_contract]
pub trait OracleAdapter {
    /// Latest accepted price for `pair`, fixed-point at [`PRICE_SCALE`]. Reverts if
    /// unset or stale.
    fn latest_price(&self, pair: String) -> PriceData;

    /// Raw stored price for `pair`, or `None` if the pair has never been priced.
    /// Never reverts on staleness — the caller decides whether the price is fresh.
    fn get_price(&self, pair: String) -> Option<PriceData>;
}
