//! Error codes raised by the [`FeeModule`](crate::fees::FeeModule).

use odra::prelude::*;

/// Failures the fee module can revert with.
///
/// Discriminants are stable on-chain identifiers; do not reorder or renumber.
#[odra::odra_error]
pub enum Error {
    /// Caller does not hold the role required for this action.
    Unauthorized = 1,
    /// The supplied fee rate exceeds [`crate::fees::MAX_FEE_BPS`].
    FeeRateTooHigh = 2,
    /// A fee or withdrawal computation overflowed `U512`.
    Overflow = 3,
    /// A withdrawal was requested but the account has nothing accrued.
    NothingAccrued = 4,
    /// An accrual was attempted with a zero notional amount.
    ZeroAmount = 5,
}
