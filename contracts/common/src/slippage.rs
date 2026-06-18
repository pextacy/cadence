//! Slippage predicate, extracted verbatim from the vault's `execute_slice`.
//!
//! The vault accepts a slice when the implied slippage between the venue quote
//! (`quoted_out`) and the agent's committed floor (`min_out`) is within the
//! mandate's `max_slippage_bps`. It avoids floating point by cross-multiplying
//! (`vault.rs:454-459`):
//!
//! ```text
//! lhs = (quoted_out - min_out) * BPS_DENOMINATOR
//! rhs = quoted_out * max_slippage_bps
//! accept  iff  lhs <= rhs
//! ```
//!
//! The vault also rejects `min_out > quoted_out` up front (`vault.rs:445`,
//! `Error::MinOutAboveQuote`); [`check_slippage`] surfaces that as
//! [`SlippageError::MinOutAboveQuote`] so the ordering matches the contract.

use odra::casper_types::U512;

use crate::checked::{checked_mul, checked_sub, MathError};
use crate::scale::BPS_DENOMINATOR;

/// Why a slice's slippage check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlippageError {
    /// `min_out > quoted_out` — nonsensical (vault `Error::MinOutAboveQuote`).
    MinOutAboveQuote,
    /// Implied slippage exceeds the cap (vault `Error::SlippageTooHigh`).
    SlippageTooHigh,
    /// Overflow while cross-multiplying the bps comparison.
    Math(MathError),
}

impl From<MathError> for SlippageError {
    fn from(e: MathError) -> Self {
        SlippageError::Math(e)
    }
}

/// `true` iff `(quoted_out - min_out) * BPS_DENOMINATOR <= quoted_out * max_bps`,
/// the raw predicate the vault uses. Assumes `min_out <= quoted_out` (callers
/// should use [`check_slippage`] which enforces that first).
pub fn within_slippage(
    quoted_out: U512,
    min_out: U512,
    max_slippage_bps: u32,
) -> Result<bool, MathError> {
    let diff = checked_sub(quoted_out, min_out)?;
    let lhs = checked_mul(diff, U512::from(BPS_DENOMINATOR))?;
    let rhs = checked_mul(quoted_out, U512::from(u64::from(max_slippage_bps)))?;
    Ok(lhs <= rhs)
}

/// Full vault-order slippage guard: reject `min_out > quoted_out`, then enforce
/// the bps cap. `Ok(())` means the slice would pass `execute_slice`'s slippage
/// stage.
pub fn check_slippage(
    quoted_out: U512,
    min_out: U512,
    max_slippage_bps: u32,
) -> Result<(), SlippageError> {
    if min_out > quoted_out {
        return Err(SlippageError::MinOutAboveQuote);
    }
    if within_slippage(quoted_out, min_out, max_slippage_bps)? {
        Ok(())
    } else {
        Err(SlippageError::SlippageTooHigh)
    }
}
