//! Events emitted by the [`TreasuryMultisig`](crate::multisig::TreasuryMultisig).

use odra::prelude::*;

/// Emitted when a new proposal is created.
#[odra::event]
pub struct ProposalCreated {
    /// Dense id assigned to the new proposal.
    pub id: u64,
    /// The owner that created it.
    pub proposer: Address,
    /// The 32-byte commitment to the action being authorised.
    pub action_hash: [u8; 32],
}

/// Emitted when an owner approves a proposal.
#[odra::event]
pub struct Approved {
    /// Id of the approved proposal.
    pub id: u64,
    /// The owner that approved.
    pub owner: Address,
    /// Total distinct approvals after this approval.
    pub approvals: u32,
}

/// Emitted when an owner withdraws a previously cast approval.
#[odra::event]
pub struct Revoked {
    /// Id of the affected proposal.
    pub id: u64,
    /// The owner that revoked.
    pub owner: Address,
    /// Total distinct approvals after this revocation.
    pub approvals: u32,
}

/// Emitted when a proposal reaches threshold and is executed (once).
#[odra::event]
pub struct Executed {
    /// Id of the executed proposal.
    pub id: u64,
    /// The 32-byte commitment to the action that was authorised.
    pub action_hash: [u8; 32],
    /// Distinct approvals at execution time.
    pub approvals: u32,
}
