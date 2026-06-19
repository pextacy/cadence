//! Thin deployable wrapper exposing the RBAC + pausable primitives standalone.

use crate::access_control::AccessControl;
use crate::pausable::PausableGuardian;
use crate::roles::{self, Role};
use odra::prelude::*;

/// Thin deployable wrapper exposing
/// [`AccessControl`](crate::access_control::AccessControl) +
/// [`PausableGuardian`](crate::pausable::PausableGuardian) as a standalone
/// contract.
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
        self.ac
            .grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
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
