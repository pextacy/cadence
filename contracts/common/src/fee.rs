//! Pure basis-points fee math (new — the fee module will build on this).
//!
//! A fee is `amount * bps / BPS_DENOMINATOR`, computed with checked
//! multiplication so a large `amount` cannot overflow silently. Integer division
//! truncates toward zero, so the fee is always rounded down (never over-charges).

use odra::casper_types::U512;

use crate::checked::{checked_div, checked_mul, checked_sub, MathError};
use crate::scale::BPS_DENOMINATOR;

/// `amount * bps / BPS_DENOMINATOR`, rounded down. `bps` is the fee rate in basis
/// points (e.g. `25` == 0.25%). Multiplication is checked; the denominator is the
/// non-zero constant [`BPS_DENOMINATOR`].
pub fn fee_amount(amount: U512, bps: u32) -> Result<U512, MathError> {
    let scaled = checked_mul(amount, U512::from(u64::from(bps)))?;
    checked_div(scaled, U512::from(BPS_DENOMINATOR))
}

/// The amount left after deducting [`fee_amount`]: `amount - fee`. Because the fee
/// is rounded down it is always `<= amount`, so this never underflows for
/// `bps <= BPS_DENOMINATOR`; the checked subtraction defends the degenerate
/// `bps > BPS_DENOMINATOR` case.
pub fn net_after_fee(amount: U512, bps: u32) -> Result<U512, MathError> {
    let fee = fee_amount(amount, bps)?;
    checked_sub(amount, fee)
}
