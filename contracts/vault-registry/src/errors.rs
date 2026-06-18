//! Error codes raised by the [`VaultRegistry`](crate::registry::VaultRegistry).

use odra::prelude::*;

/// Failures the registry can revert with.
///
/// Discriminants are stable on-chain identifiers; do not reorder or renumber.
#[odra::odra_error]
pub enum Error {
    /// Caller does not hold a role authorised to write to the registry.
    Unauthorized = 1,
    /// No record exists for the supplied registry id.
    UnknownVault = 2,
    /// The vault address has already been registered.
    AlreadyRegistered = 3,
    /// The requested status change is not a legal lifecycle transition.
    InvalidStatusTransition = 4,
    /// The registry id counter overflowed `u64`.
    Overflow = 5,
}
