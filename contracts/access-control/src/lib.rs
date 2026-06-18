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

pub mod roles;

use odra::prelude::*;
use roles::Role;

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

/// Emitted when a role is granted to an account.
#[odra::event]
pub struct RoleGranted {
    pub role: Role,
    pub account: Address,
    pub sender: Address,
}

/// Emitted when a role is revoked from an account.
#[odra::event]
pub struct RoleRevoked {
    pub role: Role,
    pub account: Address,
    pub sender: Address,
}

/// Emitted when the admin role of a role is changed.
#[odra::event]
pub struct RoleAdminChanged {
    pub role: Role,
    pub previous_admin: Role,
    pub new_admin: Role,
}

/// Emitted when a two-step role transfer is initiated.
#[odra::event]
pub struct RoleTransferStarted {
    pub role: Role,
    pub to: Address,
    pub sender: Address,
}

/// Emitted when a two-step role transfer is accepted by the recipient.
#[odra::event]
pub struct RoleTransferAccepted {
    pub role: Role,
    pub account: Address,
}

/// Emitted on pause / unpause.
#[odra::event]
pub struct PausedChanged {
    pub paused: bool,
    pub account: Address,
}

/// Reusable role-based access-control sub-module.
///
/// Compose into another module with `SubModule<AccessControl>`; the composing
/// contract supplies the caller and decides which entrypoints require which
/// role via [`AccessControl::assert_role`].
#[odra::module(
    events = [RoleGranted, RoleRevoked, RoleAdminChanged, RoleTransferStarted, RoleTransferAccepted],
    errors = Error
)]
pub struct AccessControl {
    /// `(role, account) -> has_role`.
    roles: Mapping<(Role, Address), bool>,
    /// `role -> admin_role`. Unset entries default to [`roles::ROOT_ADMIN`].
    role_admin: Mapping<Role, Role>,
    /// `role -> pending recipient` for two-step transfer. The value is itself an
    /// `Option` so the pending entry can be cleared (`set(&role, None)`) on
    /// accept, distinct from "never set".
    pending_role_transfer: Mapping<Role, Option<Address>>,
}

#[odra::module]
impl AccessControl {
    /// Whether `who` currently holds `role`.
    pub fn has_role(&self, role: Role, who: Address) -> bool {
        self.roles.get_or_default(&(role, who))
    }

    /// The admin role that administers `role`. Defaults to
    /// [`roles::ROOT_ADMIN`] when unset.
    pub fn get_role_admin(&self, role: Role) -> Role {
        match self.role_admin.get(&role) {
            Some(admin) => admin,
            None => roles::ROOT_ADMIN,
        }
    }

    /// The pending recipient of a two-step transfer of `role`, if any.
    pub fn pending_role(&self, role: Role) -> Option<Address> {
        self.pending_role_transfer.get(&role).flatten()
    }

    /// Revert with [`Error::Unauthorized`] unless `who` holds `role`.
    pub fn assert_role(&self, role: Role, who: Address) {
        if !self.has_role(role, who) {
            self.env().revert(Error::Unauthorized);
        }
    }

    /// Grant `role` to `who`. The caller MUST hold `role_admin[role]`.
    pub fn grant_role(&mut self, role: Role, who: Address) {
        let caller = self.env().caller();
        self.assert_role_admin(role, caller);
        self.grant_unchecked(role, who, caller);
    }

    /// Revoke `role` from `who`. The caller MUST hold `role_admin[role]`.
    pub fn revoke_role(&mut self, role: Role, who: Address) {
        let caller = self.env().caller();
        self.assert_role_admin(role, caller);
        if self.has_role(role, who) {
            self.roles.set(&(role, who), false);
            self.env().emit_event(RoleRevoked { role, account: who, sender: caller });
        }
    }

    /// Begin a two-step transfer of `role` to `to`. The caller MUST currently
    /// hold `role`. The recipient must later call [`AccessControl::accept_role`].
    /// The caller retains the role until the transfer is accepted.
    pub fn begin_transfer_role(&mut self, role: Role, to: Address) {
        let caller = self.env().caller();
        self.assert_role(role, caller);
        self.pending_role_transfer.set(&role, Some(to));
        self.env().emit_event(RoleTransferStarted { role, to, sender: caller });
    }

    /// Accept a pending two-step transfer of `role`. The caller MUST be the
    /// recipient set by [`AccessControl::begin_transfer_role`]. Grants `role` to
    /// the caller and clears the pending entry.
    ///
    /// Note: this does not auto-revoke the previous holder — the role admin (or
    /// the previous holder, via `revoke_role`) controls revocation. This keeps
    /// the primitive flexible for both "rotate" (admin revokes old) and "add a
    /// co-holder" semantics; the vault's `rotate_agent` revokes the old holder.
    pub fn accept_role(&mut self, role: Role) {
        let caller = self.env().caller();
        let pending = self
            .pending_role_transfer
            .get(&role)
            .flatten()
            .unwrap_or_revert_with(&self.env(), Error::NoPendingTransfer);
        if pending != caller {
            self.env().revert(Error::NotPendingRecipient);
        }
        self.pending_role_transfer.set(&role, None);
        self.grant_unchecked(role, caller, caller);
        self.env().emit_event(RoleTransferAccepted { role, account: caller });
    }

    /// Set the admin role for `role`. The caller MUST hold the CURRENT admin of
    /// `role`. Allows re-delegating administration away from `ROOT_ADMIN`.
    pub fn set_role_admin(&mut self, role: Role, new_admin: Role) {
        let caller = self.env().caller();
        self.assert_role_admin(role, caller);
        let previous_admin = self.get_role_admin(role);
        self.role_admin.set(&role, new_admin);
        self.env().emit_event(RoleAdminChanged { role, previous_admin, new_admin });
    }

    // ----- internal helpers -----

    /// Grant `role` to `who` WITHOUT an admin check. Used by `init`-style
    /// bootstrap on the composing contract and by [`AccessControl::accept_role`].
    /// NOT exposed as an entrypoint.
    pub fn grant_unchecked(&mut self, role: Role, who: Address, sender: Address) {
        if !self.has_role(role, who) {
            self.roles.set(&(role, who), true);
            self.env().emit_event(RoleGranted { role, account: who, sender });
        }
    }

    /// Set `role`'s admin WITHOUT a caller check. Used only during the composing
    /// contract's `init` to wire up the initial admin topology.
    pub fn set_role_admin_unchecked(&mut self, role: Role, admin: Role) {
        self.role_admin.set(&role, admin);
    }

    fn assert_role_admin(&self, role: Role, who: Address) {
        let admin = self.get_role_admin(role);
        if !self.has_role(admin, who) {
            self.env().revert(Error::NotRoleAdmin);
        }
    }
}

/// Reusable pausable guardian sub-module: a single pause flag with guards.
///
/// The composing contract gates `set_paused` behind the `GUARDIAN` role (via
/// [`AccessControl::assert_role`]) and calls [`PausableGuardian::assert_not_paused`]
/// on fund-moving entrypoints. Kept separate from [`AccessControl`] so a
/// contract can adopt RBAC without a pause flag, or vice versa.
#[odra::module(events = [PausedChanged], errors = Error)]
pub struct PausableGuardian {
    paused: Var<bool>,
}

#[odra::module]
impl PausableGuardian {
    /// Whether the contract is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.get_or_default()
    }

    /// Revert with [`Error::Paused`] if currently paused.
    pub fn assert_not_paused(&self) {
        if self.is_paused() {
            self.env().revert(Error::Paused);
        }
    }

    /// Revert with [`Error::NotPaused`] if currently un-paused.
    pub fn assert_paused(&self) {
        if !self.is_paused() {
            self.env().revert(Error::NotPaused);
        }
    }

    /// Set the pause flag and emit [`PausedChanged`]. The composing contract is
    /// responsible for asserting the caller holds the `GUARDIAN` role BEFORE
    /// calling this (the sub-module has no notion of roles by design).
    pub fn set_paused(&mut self, paused: bool, account: Address) {
        self.paused.set(paused);
        self.env().emit_event(PausedChanged { paused, account });
    }
}

/// Thin deployable wrapper exposing [`AccessControl`] + [`PausableGuardian`] as
/// a standalone contract.
///
/// This is what `Odra.toml` points at so the crate mirrors the `cep18` build
/// layout. In production the vault/factory compose the sub-modules directly
/// rather than calling this contract cross-contract, but this wrapper makes the
/// RBAC logic independently deployable, schema-able, and integration-testable.
///
/// The deployer becomes the [`roles::ROOT_ADMIN`] holder and can grant any role.
#[odra::module]
pub struct AccessControlContract {
    ac: SubModule<AccessControl>,
    pausable: SubModule<PausableGuardian>,
}

#[odra::module]
impl AccessControlContract {
    /// Bootstrap: the deployer becomes the root admin and is granted
    /// [`roles::TREASURY`] and [`roles::GUARDIAN`] as a sensible default
    /// administrative footing.
    pub fn init(&mut self) {
        let deployer = self.env().caller();
        self.ac.grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
        self.ac.grant_unchecked(roles::TREASURY, deployer, deployer);
        self.ac.grant_unchecked(roles::GUARDIAN, deployer, deployer);
    }

    pub fn has_role(&self, role: Role, who: Address) -> bool {
        self.ac.has_role(role, who)
    }

    pub fn assert_role(&self, role: Role, who: Address) {
        self.ac.assert_role(role, who);
    }

    pub fn grant_role(&mut self, role: Role, who: Address) {
        self.ac.grant_role(role, who);
    }

    pub fn revoke_role(&mut self, role: Role, who: Address) {
        self.ac.revoke_role(role, who);
    }

    pub fn begin_transfer_role(&mut self, role: Role, to: Address) {
        self.ac.begin_transfer_role(role, to);
    }

    pub fn accept_role(&mut self, role: Role) {
        self.ac.accept_role(role);
    }

    pub fn set_role_admin(&mut self, role: Role, new_admin: Role) {
        self.ac.set_role_admin(role, new_admin);
    }

    pub fn get_role_admin(&self, role: Role) -> Role {
        self.ac.get_role_admin(role)
    }

    pub fn pending_role(&self, role: Role) -> Option<Address> {
        self.ac.pending_role(role)
    }

    /// Guardian-gated pause toggle. Caller MUST hold [`roles::GUARDIAN`].
    pub fn set_paused(&mut self, paused: bool) {
        let caller = self.env().caller();
        self.ac.assert_role(roles::GUARDIAN, caller);
        self.pausable.set_paused(paused, caller);
    }

    pub fn is_paused(&self) -> bool {
        self.pausable.is_paused()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostEnv, NoArgs};

    struct Fixture {
        env: HostEnv,
        contract: AccessControlContractHostRef,
        admin: Address,
        alice: Address,
        bob: Address,
    }

    fn setup() -> Fixture {
        let env = odra_test::env();
        let admin = env.get_account(0);
        let alice = env.get_account(1);
        let bob = env.get_account(2);
        env.set_caller(admin);
        let contract = AccessControlContract::deploy(&env, NoArgs);
        Fixture { env, contract, admin, alice, bob }
    }

    #[test]
    fn deployer_is_root_admin_and_has_defaults() {
        let fx = setup();
        assert!(fx.contract.has_role(roles::ROOT_ADMIN, fx.admin));
        assert!(fx.contract.has_role(roles::TREASURY, fx.admin));
        assert!(fx.contract.has_role(roles::GUARDIAN, fx.admin));
        assert!(!fx.contract.has_role(roles::AGENT, fx.admin));
    }

    #[test]
    fn root_admin_can_grant_any_role() {
        let mut fx = setup();
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::AGENT, fx.alice);
        assert!(fx.contract.has_role(roles::AGENT, fx.alice));
    }

    #[test]
    fn non_admin_cannot_grant_role() {
        let mut fx = setup();
        fx.env.set_caller(fx.alice); // alice holds nothing
        let err = fx.contract.try_grant_role(roles::AGENT, fx.bob).unwrap_err();
        assert_eq!(err, Error::NotRoleAdmin.into());
        assert!(!fx.contract.has_role(roles::AGENT, fx.bob));
    }

    #[test]
    fn revoke_removes_the_role() {
        let mut fx = setup();
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::AGENT, fx.alice);
        assert!(fx.contract.has_role(roles::AGENT, fx.alice));
        fx.contract.revoke_role(roles::AGENT, fx.alice);
        assert!(!fx.contract.has_role(roles::AGENT, fx.alice));
    }

    #[test]
    fn non_admin_cannot_revoke() {
        let mut fx = setup();
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::AGENT, fx.alice);
        fx.env.set_caller(fx.bob);
        let err = fx.contract.try_revoke_role(roles::AGENT, fx.alice).unwrap_err();
        assert_eq!(err, Error::NotRoleAdmin.into());
        assert!(fx.contract.has_role(roles::AGENT, fx.alice));
    }

    #[test]
    fn assert_role_reverts_for_non_holder() {
        let fx = setup();
        let err = fx.contract.try_assert_role(roles::TREASURY, fx.alice).unwrap_err();
        assert_eq!(err, Error::Unauthorized.into());
    }

    #[test]
    fn assert_role_passes_for_holder() {
        let fx = setup();
        // admin holds TREASURY by default — assert must not revert.
        fx.contract.assert_role(roles::TREASURY, fx.admin);
    }

    #[test]
    fn two_step_transfer_grants_to_recipient_on_accept() {
        let mut fx = setup();
        // Give alice the AGENT role first so she can transfer it.
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::AGENT, fx.alice);

        fx.env.set_caller(fx.alice);
        fx.contract.begin_transfer_role(roles::AGENT, fx.bob);
        assert_eq!(fx.contract.pending_role(roles::AGENT), Some(fx.bob));
        // Not granted until accepted.
        assert!(!fx.contract.has_role(roles::AGENT, fx.bob));

        fx.env.set_caller(fx.bob);
        fx.contract.accept_role(roles::AGENT);
        assert!(fx.contract.has_role(roles::AGENT, fx.bob));
        assert_eq!(fx.contract.pending_role(roles::AGENT), None);
    }

    #[test]
    fn only_pending_recipient_may_accept() {
        let mut fx = setup();
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::AGENT, fx.alice);
        fx.env.set_caller(fx.alice);
        fx.contract.begin_transfer_role(roles::AGENT, fx.bob);
        // admin (not the pending recipient) tries to accept.
        fx.env.set_caller(fx.admin);
        let err = fx.contract.try_accept_role(roles::AGENT).unwrap_err();
        assert_eq!(err, Error::NotPendingRecipient.into());
    }

    #[test]
    fn accept_without_pending_reverts() {
        let mut fx = setup();
        fx.env.set_caller(fx.bob);
        let err = fx.contract.try_accept_role(roles::AGENT).unwrap_err();
        assert_eq!(err, Error::NoPendingTransfer.into());
    }

    #[test]
    fn begin_transfer_requires_holding_the_role() {
        let mut fx = setup();
        fx.env.set_caller(fx.alice); // alice does not hold AGENT
        let err = fx
            .contract
            .try_begin_transfer_role(roles::AGENT, fx.bob)
            .unwrap_err();
        assert_eq!(err, Error::Unauthorized.into());
    }

    #[test]
    fn set_role_admin_delegates_administration() {
        let mut fx = setup();
        // Make TREASURY the admin of AGENT. admin holds TREASURY + ROOT_ADMIN,
        // and ROOT_ADMIN is the current admin of AGENT, so admin may set it.
        fx.env.set_caller(fx.admin);
        fx.contract.set_role_admin(roles::AGENT, roles::TREASURY);
        assert_eq!(fx.contract.get_role_admin(roles::AGENT), roles::TREASURY);

        // Now grant TREASURY to alice; alice (a treasury) may grant AGENT.
        fx.contract.grant_role(roles::TREASURY, fx.alice);
        fx.env.set_caller(fx.alice);
        fx.contract.grant_role(roles::AGENT, fx.bob);
        assert!(fx.contract.has_role(roles::AGENT, fx.bob));
    }

    #[test]
    fn guardian_can_pause_and_unpause() {
        let mut fx = setup();
        assert!(!fx.contract.is_paused());
        fx.env.set_caller(fx.admin); // admin holds GUARDIAN by default
        fx.contract.set_paused(true);
        assert!(fx.contract.is_paused());
        fx.contract.set_paused(false);
        assert!(!fx.contract.is_paused());
    }

    #[test]
    fn non_guardian_cannot_pause() {
        let mut fx = setup();
        fx.env.set_caller(fx.alice); // alice is not a guardian
        let err = fx.contract.try_set_paused(true).unwrap_err();
        assert_eq!(err, Error::Unauthorized.into());
        assert!(!fx.contract.is_paused());
    }

    #[test]
    fn granted_guardian_can_pause() {
        let mut fx = setup();
        fx.env.set_caller(fx.admin);
        fx.contract.grant_role(roles::GUARDIAN, fx.alice);
        fx.env.set_caller(fx.alice);
        fx.contract.set_paused(true);
        assert!(fx.contract.is_paused());
    }
}
