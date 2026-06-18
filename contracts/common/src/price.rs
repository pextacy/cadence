//! Fixed-point price derivation and band check, extracted from the vault.
//!
//! The vault derives the implied price of a slice as buy-asset units per one
//! sell-asset unit, scaled by `PRICE_SCALE` (`vault.rs:463`):
//!
//! ```text
//! price = quoted_out * PRICE_SCALE / sell_amount
//! ```
//!
//! and accepts it when it sits inside the mandate band, where a `0` bound means
//! "unset / no bound" (`vault.rs:464-471`):
//!
//! ```text
//! accept iff (floor == 0   || price >= floor)
//!         && (ceiling == 0 || price <= ceiling)
//! ```

use odra::casper_types::U512;

use crate::checked::{checked_div, checked_mul, MathError};
use crate::scale::PRICE_SCALE;

/// Why a slice's price-band check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceError {
    /// Derived price is below the floor or above the ceiling
    /// (vault `Error::PriceOutOfBand`).
    OutOfBand,
    /// Overflow / divide-by-zero deriving the price (e.g. `sell_amount == 0`).
    Math(MathError),
}

impl From<MathError> for PriceError {
    fn from(e: MathError) -> Self {
        PriceError::Math(e)
    }
}

/// `quoted_out * scale / sell_amount` — the implied price at an arbitrary scale.
/// `sell_amount == 0` yields [`MathError::DivByZero`] (the vault guards against a
/// zero `sell_amount` before this point, so it never divides by zero in practice).
pub fn implied_price_scaled(
    quoted_out: U512,
    sell_amount: U512,
    scale: u64,
) -> Result<U512, MathError> {
    let scaled = checked_mul(quoted_out, U512::from(scale))?;
    checked_div(scaled, sell_amount)
}

/// The vault's price: `quoted_out * PRICE_SCALE / sell_amount` (1e9 scale).
pub fn implied_price(quoted_out: U512, sell_amount: U512) -> Result<U512, MathError> {
    implied_price_scaled(quoted_out, sell_amount, PRICE_SCALE)
}

/// `true` iff `price` is within `[floor, ceiling]`, treating a `0` bound as unset.
/// Pure comparison, no arithmetic — mirrors `vault.rs:464-471` exactly.
pub fn within_band(price: U512, floor: U512, ceiling: U512) -> bool {
    if !floor.is_zero() && price < floor {
        return false;
    }
    if !ceiling.is_zero() && price > ceiling {
        return false;
    }
    true
}

/// Derive the vault price for a slice and check it against the mandate band.
/// `Ok(())` means the slice would pass `execute_slice`'s price stage.
pub fn check_price_band(
    quoted_out: U512,
    sell_amount: U512,
    floor: U512,
    ceiling: U512,
) -> Result<(), PriceError> {
    let price = implied_price(quoted_out, sell_amount)?;
    if within_band(price, floor, ceiling) {
        Ok(())
    } else {
        Err(PriceError::OutOfBand)
    }
}
