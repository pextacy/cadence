//! Value types stored and returned by the
//! [`TreasuryMultisig`](crate::multisig::TreasuryMultisig).
//!
//! A [`Proposal`] is the on-chain record of one pending treasury action: the
//! dense `u64` id assigned at creation, the account that proposed it, the 32-byte
//! `action_hash` committing to the off-chain action payload, the running tally of
//! distinct owner approvals, the block-time it was created, and its lifecycle
//! [`ProposalStatus`]. Every field except `approvals` and `status` is set once at
//! creation and never mutated.

use odra::prelude::*;

/// Lifecycle state of a proposal.
///
/// Modelled as an explicit enum so illegal states are unrepresentable and every
/// consumer matches exhaustively. A proposal is created [`Pending`](Self::Pending),
/// gathers approvals while pending, and on reaching the threshold is
/// [`execute`](crate::multisig::TreasuryMultisig::execute)d exactly once — moving
/// it to the terminal [`Executed`](Self::Executed) state. Execution is the only
/// terminal transition; there is no cancellation.
#[odra::odra_type]
pub enum ProposalStatus {
    /// Open for approvals; not yet executed.
    Pending,
    /// Threshold met and the action has been executed. Terminal.
    Executed,
}

impl ProposalStatus {
    /// Whether the proposal is still open for approvals / revocations.
    pub fn is_pending(&self) -> bool {
        matches!(self, ProposalStatus::Pending)
    }

    /// Whether the proposal has already been executed (its terminal state).
    pub fn is_executed(&self) -> bool {
        matches!(self, ProposalStatus::Executed)
    }
}

/// One pending (or executed) M-of-N treasury action.
///
/// `id` is the multisig's own dense `u64` key, assigned at creation. `action_hash`
/// is an opaque 32-byte commitment to the action payload the owners are authorising
/// (e.g. a hash of the call the treasury will perform off this approval); the
/// multisig treats it purely as an identifier and never interprets it. `approvals`
/// is the count of *distinct* current owner approvals — it can decrease via
/// [`revoke`](crate::multisig::TreasuryMultisig::revoke).
#[odra::odra_type]
pub struct Proposal {
    /// Dense proposal id assigned at creation (the `Mapping` key).
    pub id: u64,
    /// The owner account that created the proposal.
    pub proposer: Address,
    /// The 32-byte commitment to the action being authorised.
    pub action_hash: [u8; 32],
    /// Count of distinct current owner approvals.
    pub approvals: u32,
    /// Block-time (ms since epoch, per the host env) the proposal was created.
    pub created_at: u64,
    /// Current lifecycle status.
    pub status: ProposalStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_predicates() {
        assert!(ProposalStatus::Pending.is_pending());
        assert!(!ProposalStatus::Pending.is_executed());
    }

    #[test]
    fn executed_predicates() {
        assert!(ProposalStatus::Executed.is_executed());
        assert!(!ProposalStatus::Executed.is_pending());
    }
}
