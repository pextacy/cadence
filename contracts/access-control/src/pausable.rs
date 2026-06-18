//! The shared pausable guardian sub-module.

use crate::errors::Error;
use crate::events::PausedChanged;
use odra::prelude::*;

/// Reusable pausable guardian sub-module: a single pause flag with guards.
///
/// The composing contract gates `set_paused` behind the `GUARDIAN` role (via
/// [`AccessControl::assert_role`](crate::access_control::AccessControl::assert_role))
/// and calls [`PausableGuardian::assert_not_paused`] on fund-moving entrypoints.
/// Kept separate from
/// [`AccessControl`](crate::access_control::AccessControl) so a contract can
/// adopt RBAC without a pause flag, or vice versa.
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
