//! Error codes raised by the [`Guardian`](crate::guardian::Guardian).
//!
//! Discriminants are stable on-chain identifiers; do not reorder or renumber.

use odra::prelude::*;

/// Failures the guardian can revert with.
#[odra::odra_error]
pub enum Error {
    /// Caller does not hold the `GUARDIAN` role required for this action.
    Unauthorized = 1,
    /// A global pause is already engaged (or already lifted) — the requested
    /// transition is a no-op and is rejected so the desk-wide flag is never
    /// toggled redundantly.
    AlreadyInState = 2,
    /// A pagination bound was zero or exceeded the per-batch fan-out cap, which
    /// would risk an unbounded / out-of-gas sweep.
    InvalidBatchBound = 3,
}

/// Hard ceiling on how many vaults a single `global_pause` / `global_resume`
/// call may fan out to. Bounds the cross-contract sweep so one transaction can
/// never run unbounded gas; larger desks paginate across multiple calls.
pub const MAX_FANOUT_PER_CALL: u64 = 64;
