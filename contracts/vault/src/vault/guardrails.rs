//! Pure guardrail predicates for `execute_slice`.
//!
//! Every function here is environment-free: it takes values and returns a
//! `Result<_, Error>`, delegating the actual arithmetic to the audited
//! [`cadence_common`] math library. The contract layer (`execution.rs`) calls
//! these and reverts on `Err`, so the guardrail logic is identical to the inline
//! version it replaced — see the per-function references to `vault.rs`.

use odra::casper_types::U512;

use cadence_common::checked::{checked_add, MathError};
use cadence_common::price::{check_price_band, PriceError};
use cadence_common::slippage::{check_slippage, SlippageError};

use super::errors::Error;

/// Map a raw [`MathError`] to the vault's overflow revert. Underflow / div-by-zero
/// cannot occur on the paths that use this (the vault guards them structurally),
/// so they collapse to the same `Overflow` reason the inline code used.
fn map_math(_e: MathError) -> Error {
    Error::Overflow
}

/// `sold_so_far + sell_amount`, checked. Mirrors `vault.rs:449`.
pub fn add_sold(sold_so_far: U512, sell_amount: U512) -> Result<U512, Error> {
    checked_add(sold_so_far, sell_amount).map_err(map_math)
}

/// `bought_so_far + bought_amount`, checked. Mirrors `vault.rs:520`.
pub fn add_bought(bought_so_far: U512, bought_amount: U512) -> Result<U512, Error> {
    checked_add(bought_so_far, bought_amount).map_err(map_math)
}

/// Reject a new cumulative sold size that exceeds the mandate cap.
/// Mirrors `vault.rs:450-452`.
pub fn check_spend_cap(new_sold: U512, total_sell: U512) -> Result<(), Error> {
    if new_sold > total_sell {
        return Err(Error::SpendCapExceeded);
    }
    Ok(())
}

/// Slippage guard: reject `min_out > quoted_out`, then enforce the bps cap.
/// Mirrors `vault.rs:445-460`, delegating to [`check_slippage`].
pub fn check_slice_slippage(
    quoted_out: U512,
    min_out: U512,
    max_slippage_bps: u32,
) -> Result<(), Error> {
    check_slippage(quoted_out, min_out, max_slippage_bps).map_err(|e| match e {
        SlippageError::MinOutAboveQuote => Error::MinOutAboveQuote,
        SlippageError::SlippageTooHigh => Error::SlippageTooHigh,
        SlippageError::Math(_) => Error::Overflow,
    })
}

/// Price-band guard: derive the slice price and check it against `[floor, ceiling]`
/// (a `0` bound is unset). Mirrors `vault.rs:462-471`, delegating to
/// [`check_price_band`].
pub fn check_slice_price(
    quoted_out: U512,
    sell_amount: U512,
    price_floor: U512,
    price_ceiling: U512,
) -> Result<(), Error> {
    check_price_band(quoted_out, sell_amount, price_floor, price_ceiling).map_err(|e| match e {
        PriceError::OutOfBand => Error::PriceOutOfBand,
        PriceError::Math(_) => Error::Overflow,
    })
}

/// Reconcile a realised fill against the slice's committed `min_out`.
/// Mirrors `vault.rs:516-519`.
pub fn check_fill_min_out(bought_amount: U512, min_out: U512) -> Result<(), Error> {
    if bought_amount < min_out {
        return Err(Error::SlippageTooHigh);
    }
    Ok(())
}
