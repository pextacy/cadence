//! # Cadence Vault Registry
//!
//! An authenticated on-chain index of deployed Cadence execution vaults. The
//! factory registers each vault it deploys here; off-chain tooling and the
//! guardian enumerate vaults and reflect lifecycle status. The registry never
//! moves funds — it only indexes.
//!
//! ## Layout
//!
//! * [`types`] — [`VaultRecord`](types::VaultRecord) and the
//!   [`VaultStatus`](types::VaultStatus) lifecycle enum.
//! * [`errors`] — the [`Error`](errors::Error) code set.
//! * [`events`] — [`VaultRegistered`](events::VaultRegistered) and
//!   [`VaultStatusChanged`](events::VaultStatusChanged).
//! * [`registry`] — the [`VaultRegistry`](registry::VaultRegistry) module, its
//!   storage and entrypoints, and the
//!   [`VaultRegistration`](registry::VaultRegistration) cross-contract trait.
//!
//! Writes are gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module; the
//! writer role is
//! [`FACTORY_ADMIN`](cadence_access_control::roles::FACTORY_ADMIN).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod errors;
pub mod events;
pub mod registry;
pub mod types;

pub use registry::VaultRegistry;
