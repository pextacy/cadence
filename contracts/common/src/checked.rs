//! Checked `U512` arithmetic returning [`MathError`] instead of panicking.
//!
//! The vault performs add/mul through `checked_add` / `checked_mul` helpers that
//! revert with `Error::Overflow` (`vault.rs:771-783`). Here the same operations
//! return a `Result` so the pure layer stays environment-free; the contract layer
//! maps [`MathError`] onto its own revert. Division is added for the price /
//! quote math, guarding the divide-by-zero case the vault avoids structurally.

use odra::casper_types::U512;

/// Arithmetic failure on a guardrail computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    /// An add or multiply overflowed `U512` (the vault's `Error::Overflow`).
    Overflow,
    /// A subtraction would underflow (`b > a`).
    Underflow,
    /// A division by zero was attempted.
    DivByZero,
}

/// `a + b`, erroring on overflow. Mirrors `vault.rs::checked_add`.
pub fn checked_add(a: U512, b: U512) -> Result<U512, MathError> {
    match a.checked_add(b) {
        Some(v) => Ok(v),
        None => Err(MathError::Overflow),
    }
}

/// `a - b`, erroring if `b > a`. The vault only subtracts where it has already
/// proven `b <= a` (e.g. `quoted_out - min_out` after the `min_out > quoted_out`
/// guard); this helper makes that precondition explicit and safe to reuse.
pub fn checked_sub(a: U512, b: U512) -> Result<U512, MathError> {
    match a.checked_sub(b) {
        Some(v) => Ok(v),
        None => Err(MathError::Underflow),
    }
}

/// `a * b`, erroring on overflow. Mirrors `vault.rs::checked_mul`.
pub fn checked_mul(a: U512, b: U512) -> Result<U512, MathError> {
    match a.checked_mul(b) {
        Some(v) => Ok(v),
        None => Err(MathError::Overflow),
    }
}

/// `a / b`, erroring on a zero divisor. `U512` division truncates toward zero,
/// matching the vault's integer price math (`vault.rs:463`).
pub fn checked_div(a: U512, b: U512) -> Result<U512, MathError> {
    if b.is_zero() {
        return Err(MathError::DivByZero);
    }
    Ok(a / b)
}
