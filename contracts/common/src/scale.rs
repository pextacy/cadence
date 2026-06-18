//! Fixed-point scale constants and conversions.
//!
//! ## A note on the two scales (documented, NOT unified)
//!
//! The Cadence contracts currently use **two different** fixed-point price scales,
//! and this module exposes **both** as distinct named constants. They are NOT
//! reconciled here on purpose — silently unifying them would change on-chain
//! behaviour of already-written contracts. The decompose/build agents own that
//! migration; this crate only documents reality:
//!
//! - [`PRICE_SCALE_1E9`] = `1_000_000_000` — used by `vault` and `price-oracle`.
//!   The vault's price-band check and the oracle's stored prices are at 1e9.
//! - [`PRICE_SCALE_1E6`] = `1_000_000` — used by `dex-adapter` (`cep18_swap`).
//!   The DEX swap computes `bought = sell * price / 1e6`.
//!
//! [`PRICE_SCALE`] is an alias for [`PRICE_SCALE_1E9`] so callers porting the
//! vault's `PRICE_SCALE` symbol get identical behaviour with no value change.

use odra::casper_types::U512;

use crate::checked::{checked_div, checked_mul, MathError};

/// Vault / price-oracle fixed-point scale: a price of `1.0` is `1_000_000_000`.
/// Matches `vault.rs::PRICE_SCALE` and `oracle.rs::PRICE_SCALE`.
pub const PRICE_SCALE_1E9: u64 = 1_000_000_000;

/// DEX-adapter fixed-point scale: a price of `1.0` is `1_000_000`.
/// Matches `cep18_swap.rs::PRICE_SCALE`.
pub const PRICE_SCALE_1E6: u64 = 1_000_000;

/// Canonical alias for the vault's scale. Use this when porting `vault.rs`
/// (and `price-oracle`) logic — the value is unchanged (`1e9`).
pub const PRICE_SCALE: u64 = PRICE_SCALE_1E9;

/// Basis-points denominator: `100%` is `10_000` bps. Matches
/// `vault.rs::BPS_DENOMINATOR`.
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Apply a fixed-point price to an amount: `amount * price / scale`.
///
/// This is the DEX-adapter's quote math (`cep18_swap.rs`:
/// `bought = sell_amount * price / PRICE_SCALE`) generalised over `scale`.
/// Multiplication is checked; the division denominator is the constant `scale`
/// so it can only be zero if the caller passes `0` (guarded).
pub fn apply_price(amount: U512, price: U512, scale: u64) -> Result<U512, MathError> {
    let scaled = checked_mul(amount, price)?;
    checked_div(scaled, U512::from(scale))
}

/// Convert a value expressed at `from_scale` into the equivalent value at
/// `to_scale`: `value * to_scale / from_scale`. Used to bridge the 1e9 and 1e6
/// representations explicitly when a caller really intends a conversion (it never
/// happens implicitly anywhere in this crate).
pub fn rescale(value: U512, from_scale: u64, to_scale: u64) -> Result<U512, MathError> {
    let scaled = checked_mul(value, U512::from(to_scale))?;
    checked_div(scaled, U512::from(from_scale))
}
