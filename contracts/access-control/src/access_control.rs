//! The shared role-based access-control (RBAC) sub-module.

use crate::errors::Error;
use crate::events::{
    RoleAdminChanged, RoleGranted, RoleRevoked, RoleTransferAccepted, RoleTransferStarted,
};
use crate::roles::{self, Role};
use odra::prelude::*;

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

    /// Revoke `role` from `who` WITHOUT an admin check. The composing contract is
    /// responsible for authorizing the caller (e.g. Guardian's `assert_guardian`
    /// before a self-rotation hand-off). NOT exposed as an entrypoint.
    pub fn revoke_unchecked(&mut self, role: Role, who: Address, sender: Address) {
        if self.has_role(role, who) {
            self.roles.set(&(role, who), false);
            self.env().emit_event(RoleRevoked { role, account: who, sender });
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
