//! # Cadence Access Control
//!
//! A reusable, role-based access-control (RBAC) primitive plus a pausable
//! guardian, designed to be composed into the Cadence execution vault, vault
//! factory, oracle, and venue adapters via Odra's [`SubModule`](odra::SubModule).
//!
//! ## Design
//!
//! * [`AccessControl`] is the shared RBAC sub-module: a `(role, account)`
//!   membership map, a per-role admin map (`role_admin`), and a two-step role
//!   transfer (`begin_transfer_role` / `accept_role`). It mirrors the
//!   OpenZeppelin `AccessControl` semantics adapted to Odra: every role is
//!   administered by another role; the bootstrap account holds
//!   [`roles::ROOT_ADMIN`], which administers all roles until re-delegated.
//!
//! * [`PausableGuardian`] is the shared pause primitive: a single boolean pause
//!   flag plus `assert_not_paused` / `assert_paused` guards, intended to be
//!   gated by the `GUARDIAN` role at the composing contract's entrypoints
//!   (emergency_pause / emergency_withdraw).
//!
//! Both are libraries first: the vault composes them with
//! `SubModule<AccessControl>` and `SubModule<PausableGuardian>` and supplies the
//! caller checks. A thin deployable [`AccessControlContract`] wrapper is also
//! provided so the crate mirrors the `cep18` / `x402-token` build layout
//! (`Odra.toml` fqn + `build_contract` / `build_schema` bins) and so the RBAC
//! logic can be deployed and exercised standalone.
//!
//! ## Roles
//!
//! `TREASURY`, `AGENT`, `GUARDIAN`, `FEE_COLLECTOR`, `ORACLE_OPERATOR`,
//! `FACTORY_ADMIN` (see [`roles`]). `GUARDIAN` is a NEW role distinct from
//! `AGENT` / `TREASURY`.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod access_control;
pub mod contract;
pub mod errors;
pub mod events;
pub mod pausable;
pub mod roles;

pub use access_control::AccessControl;
pub use contract::AccessControlContract;
pub use errors::Error;
pub use events::{
    PausedChanged, RoleAdminChanged, RoleGranted, RoleRevoked, RoleTransferAccepted,
    RoleTransferStarted,
};
pub use pausable::PausableGuardian;
