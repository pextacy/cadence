//! # Cadence Treasury Multisig
//!
//! An M-of-N approval gate over abstract treasury actions. Cadence treasuries use
//! it as the authorisation root for sensitive operations (funding a vault, rotating
//! a mandate, releasing settlement): an owner proposes an action — committed to as
//! an opaque 32-byte `action_hash` — owners approve it, and once a configurable
//! threshold of distinct approvals is reached the action may be executed exactly
//! once. The contract never moves funds itself; it only authorises.
//!
//! ## Layout
//!
//! * [`types`] — [`Proposal`](types::Proposal) and the
//!   [`ProposalStatus`](types::ProposalStatus) lifecycle enum.
//! * [`errors`] — the [`Error`](errors::Error) code set.
//! * [`events`] — [`ProposalCreated`](events::ProposalCreated),
//!   [`Approved`](events::Approved), [`Revoked`](events::Revoked), and
//!   [`Executed`](events::Executed).
//! * [`multisig`] — the [`TreasuryMultisig`](multisig::TreasuryMultisig) module:
//!   its storage, the `propose` / `approve` / `revoke` / `execute` flow, and the
//!   read-only views.
//!
//! The owner set and threshold are fixed at construction; rotating signers means
//! deploying a fresh multisig. Executed proposals are terminal, which makes the
//! gate replay- and double-execute-safe.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod errors;
pub mod events;
pub mod multisig;
pub mod types;

pub use multisig::TreasuryMultisig;
