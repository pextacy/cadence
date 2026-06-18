//! Error codes raised by the
//! [`TreasuryMultisig`](crate::multisig::TreasuryMultisig).

use odra::prelude::*;

/// Failures the multisig can revert with.
///
/// Discriminants are stable on-chain identifiers; do not reorder or renumber.
#[odra::odra_error]
pub enum Error {
    /// Caller is not one of the configured owners.
    NotOwner = 1,
    /// No proposal exists for the supplied id.
    UnknownProposal = 2,
    /// The caller has already approved this proposal.
    AlreadyApproved = 3,
    /// The caller has not approved this proposal, so there is nothing to revoke.
    NotApproved = 4,
    /// The proposal has already been executed; it can no longer be approved,
    /// revoked, or re-executed (replay / double-execute guard).
    AlreadyExecuted = 5,
    /// The proposal has not yet reached the approval threshold to execute.
    ThresholdNotMet = 6,
    /// The owner set or threshold supplied at construction is invalid (empty
    /// owners, duplicate owner, zero threshold, or threshold exceeding the
    /// number of owners).
    InvalidConfiguration = 7,
    /// The proposal id counter overflowed `u64`.
    Overflow = 8,
}
