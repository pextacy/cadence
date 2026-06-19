//! The `TreasuryMultisig` contract: an M-of-N approval gate over treasury actions.
//!
//! ## Responsibility
//!
//! This module does exactly one thing: it gates abstract treasury *actions*
//! (identified by an opaque 32-byte `action_hash`) behind an M-of-N owner
//! approval workflow. An owner [`propose`](TreasuryMultisig::propose)s an action;
//! owners [`approve`](TreasuryMultisig::approve) (or later
//! [`revoke`](TreasuryMultisig::revoke)) it; once distinct approvals reach the
//! configured `threshold` any owner may [`execute`](TreasuryMultisig::execute) it
//! exactly once. The contract never moves funds itself — execution flips the
//! proposal to its terminal state and emits [`Executed`], which downstream tooling
//! (or a treasury contract that trusts this gate) acts on.
//!
//! ## Owner set and threshold
//!
//! The owner set and `threshold` are fixed at construction and validated:
//! owners must be non-empty and distinct, and `1 <= threshold <= owners.len()`.
//! Membership is held both as an ordered [`Vec<Address>`] (for enumeration) and a
//! `Mapping<Address, bool>` (for O(1) checks). The set is immutable for the life of
//! the contract — rotating signers means deploying a fresh multisig.
//!
//! ## Replay / double-execute safety
//!
//! A proposal carries a [`ProposalStatus`]; once [`execute`](TreasuryMultisig::execute)
//! moves it to [`ProposalStatus::Executed`] it is terminal. Any subsequent
//! `approve` / `revoke` / `execute` against that id reverts
//! [`Error::AlreadyExecuted`], so an authorised action cannot be replayed.

use crate::errors::Error;
use crate::events::{Approved, Executed, ProposalCreated, Revoked};
use crate::types::{Proposal, ProposalStatus};
use odra::prelude::*;

/// An M-of-N multisig gate over abstract treasury actions.
#[odra::module(
    events = [ProposalCreated, Approved, Revoked, Executed],
    errors = Error
)]
pub struct TreasuryMultisig {
    /// Ordered owner set (for enumeration); deduplicated at construction.
    owners: Var<Vec<Address>>,
    /// O(1) membership guard mirroring `owners`.
    is_owner: Mapping<Address, bool>,
    /// Number of distinct owner approvals required to execute (M in M-of-N).
    threshold: Var<u32>,
    /// Dense proposal id counter; also the count of proposals ever created.
    count: Var<u64>,
    /// Primary index: proposal id -> proposal record.
    proposals: Mapping<u64, Proposal>,
    /// Per-proposal, per-owner approval flag: `(id, owner) -> approved?`.
    approvals: Mapping<(u64, Address), bool>,
}

#[odra::module]
impl TreasuryMultisig {
    /// Configure the owner set and approval `threshold`.
    ///
    /// Reverts [`Error::InvalidConfiguration`] if `owners` is empty, contains a
    /// duplicate, or if `threshold` is zero or exceeds `owners.len()`.
    pub fn init(&mut self, owners: Vec<Address>, threshold: u32) {
        let len = owners.len() as u32;
        if owners.is_empty() || threshold == 0 || threshold > len {
            self.env().revert(Error::InvalidConfiguration);
        }
        // Reject duplicates while building the membership map.
        for owner in &owners {
            if self.is_owner.get_or_default(owner) {
                self.env().revert(Error::InvalidConfiguration);
            }
            self.is_owner.set(owner, true);
        }
        self.owners.set(owners);
        self.threshold.set(threshold);
        self.count.set(0);
    }

    /// Create a proposal committing to `action_hash` and return its assigned id.
    ///
    /// Caller MUST be an owner. The proposer does **not** auto-approve; approvals
    /// are cast explicitly via [`approve`](Self::approve). Reverts
    /// [`Error::Overflow`] if the id counter would wrap.
    pub fn propose(&mut self, action_hash: [u8; 32]) -> u64 {
        let proposer = self.assert_owner();

        let id = self.count.get_or_default();
        let next = match id.checked_add(1) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };

        let proposal = Proposal {
            id,
            proposer,
            action_hash,
            approvals: 0,
            created_at: self.env().get_block_time(),
            status: ProposalStatus::Pending,
        };
        self.proposals.set(&id, proposal);
        self.count.set(next);

        self.env().emit_event(ProposalCreated {
            id,
            proposer,
            action_hash,
        });
        id
    }

    /// Record the caller's approval of proposal `id`.
    ///
    /// Caller MUST be an owner. Reverts [`Error::UnknownProposal`] for an unknown
    /// id, [`Error::AlreadyExecuted`] if it is already executed, and
    /// [`Error::AlreadyApproved`] if the caller has already approved it.
    pub fn approve(&mut self, id: u64) {
        let owner = self.assert_owner();
        let proposal = self.load_pending(id);

        if self.approvals.get_or_default(&(id, owner)) {
            self.env().revert(Error::AlreadyApproved);
        }
        let approvals = match proposal.approvals.checked_add(1) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };

        self.approvals.set(&(id, owner), true);
        self.proposals.set(
            &id,
            Proposal {
                approvals,
                ..proposal
            },
        );
        self.env().emit_event(Approved {
            id,
            owner,
            approvals,
        });
    }

    /// Withdraw the caller's previously cast approval of proposal `id`.
    ///
    /// Caller MUST be an owner. Reverts [`Error::UnknownProposal`] for an unknown
    /// id, [`Error::AlreadyExecuted`] if it is already executed, and
    /// [`Error::NotApproved`] if the caller has not approved it.
    pub fn revoke(&mut self, id: u64) {
        let owner = self.assert_owner();
        let proposal = self.load_pending(id);

        if !self.approvals.get_or_default(&(id, owner)) {
            self.env().revert(Error::NotApproved);
        }
        // `approvals` is bounded below by the set of recorded approvals, so this
        // saturating subtraction can never underflow in practice; guard anyway.
        let approvals = proposal.approvals.saturating_sub(1);

        self.approvals.set(&(id, owner), false);
        self.proposals.set(
            &id,
            Proposal {
                approvals,
                ..proposal
            },
        );
        self.env().emit_event(Revoked {
            id,
            owner,
            approvals,
        });
    }

    /// Execute proposal `id` once its distinct approvals reach `threshold`.
    ///
    /// Caller MUST be an owner. Reverts [`Error::UnknownProposal`] for an unknown
    /// id, [`Error::AlreadyExecuted`] if it has already run (double-execute /
    /// replay guard), and [`Error::ThresholdNotMet`] if approvals are below the
    /// threshold. On success the proposal moves to the terminal
    /// [`ProposalStatus::Executed`] state.
    pub fn execute(&mut self, id: u64) {
        let _owner = self.assert_owner();
        let proposal = self.load_pending(id);

        let threshold = self.threshold.get_or_default();
        if proposal.approvals < threshold {
            self.env().revert(Error::ThresholdNotMet);
        }

        let action_hash = proposal.action_hash;
        let approvals = proposal.approvals;
        self.proposals.set(
            &id,
            Proposal {
                status: ProposalStatus::Executed,
                ..proposal
            },
        );
        self.env().emit_event(Executed {
            id,
            action_hash,
            approvals,
        });
    }

    // ----- read-only views -----

    /// The configured approval threshold (M in M-of-N).
    pub fn threshold(&self) -> u32 {
        self.threshold.get_or_default()
    }

    /// The ordered owner set.
    pub fn owners(&self) -> Vec<Address> {
        self.owners.get_or_default()
    }

    /// Number of owners (N in M-of-N).
    pub fn owner_count(&self) -> u32 {
        self.owners.get_or_default().len() as u32
    }

    /// Whether `who` is a configured owner.
    pub fn is_owner(&self, who: Address) -> bool {
        self.is_owner.get_or_default(&who)
    }

    /// Total number of proposals ever created (also the next id to be assigned).
    pub fn proposal_count(&self) -> u64 {
        self.count.get_or_default()
    }

    /// Fetch a proposal by id, or `None` if unknown.
    pub fn get_proposal(&self, id: u64) -> Option<Proposal> {
        self.proposals.get(&id)
    }

    /// Whether `owner` has a live approval recorded against proposal `id`.
    pub fn has_approved(&self, id: u64, owner: Address) -> bool {
        self.approvals.get_or_default(&(id, owner))
    }

    // ----- internal helpers (never exposed as entrypoints) -----

    /// Return the caller, reverting [`Error::NotOwner`] unless it is an owner.
    fn assert_owner(&self) -> Address {
        let caller = self.env().caller();
        if !self.is_owner.get_or_default(&caller) {
            self.env().revert(Error::NotOwner);
        }
        caller
    }

    /// Load a proposal that must exist and still be pending.
    ///
    /// Reverts [`Error::UnknownProposal`] if no record exists and
    /// [`Error::AlreadyExecuted`] if it is already in its terminal state.
    fn load_pending(&self, id: u64) -> Proposal {
        let proposal = self
            .proposals
            .get(&id)
            .unwrap_or_revert_with(&self.env(), Error::UnknownProposal);
        if proposal.status.is_executed() {
            self.env().revert(Error::AlreadyExecuted);
        }
        proposal
    }
}
