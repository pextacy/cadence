//! Events emitted by the access-control and pausable primitives.

use crate::roles::Role;
use odra::prelude::*;

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
