//! Errors raised by the access-control and pausable primitives.

use odra::prelude::*;

/// Errors raised by the access-control and pausable primitives.
#[odra::odra_error]
pub enum Error {
    /// The caller does not hold the role required for this action.
    Unauthorized = 1,
    /// The caller does not administer the target role (cannot grant/revoke it).
    NotRoleAdmin = 2,
    /// There is no pending transfer for this role to accept.
    NoPendingTransfer = 3,
    /// The caller is not the pending recipient of this role transfer.
    NotPendingRecipient = 4,
    /// The action requires the contract to be un-paused.
    Paused = 5,
    /// The action requires the contract to be paused.
    NotPaused = 6,
}
