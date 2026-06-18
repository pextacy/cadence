//! # Cadence Guardian
//!
//! The desk-wide kill switch for Cadence. On an incident a `GUARDIAN`-role holder
//! engages a single switch that fans out a pause to every active registered
//! vault, halting execution across the whole desk in a bounded number of bounded
//! transactions. [`Guardian::global_resume`](guardian::Guardian::global_resume)
//! lifts it the same way.
//!
//! ## Layout
//!
//! * [`errors`] — the [`Error`](errors::Error) code set plus the
//!   [`MAX_FANOUT_PER_CALL`](errors::MAX_FANOUT_PER_CALL) batch ceiling.
//! * [`events`] — [`GlobalPause`](events::GlobalPause),
//!   [`VaultPauseFanned`](events::VaultPauseFanned) and
//!   [`GuardianRotated`](events::GuardianRotated).
//! * [`storage`] — the [`VaultRegistryView`](storage::VaultRegistryView) and
//!   [`VaultControl`](storage::VaultControl) cross-contract traits the guardian
//!   dispatches through.
//! * [`guardian`] — the [`Guardian`](guardian::Guardian) module: its storage
//!   (registry address, desk-wide pause flag, RBAC sub-module) and entrypoints.
//!
//! State changes are gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module on the
//! [`GUARDIAN`](cadence_access_control::roles::GUARDIAN) role.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod errors;
pub mod events;
pub mod guardian;
pub mod storage;

pub use guardian::Guardian;
