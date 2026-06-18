//! Events emitted by the [`VaultRegistry`](crate::registry::VaultRegistry).

use crate::types::VaultStatus;
use odra::prelude::*;

/// Emitted when a new vault is indexed.
#[odra::event]
pub struct VaultRegistered {
    /// Dense registry id assigned to the new record.
    pub id: u64,
    /// The deployed vault contract's address.
    pub vault: Address,
    /// The treasury that owns / funds the vault.
    pub treasury: Address,
    /// The 32-byte mandate hash the vault executes under.
    pub mandate_hash: [u8; 32],
}

/// Emitted when a vault's lifecycle status changes.
#[odra::event]
pub struct VaultStatusChanged {
    /// Registry id of the affected record.
    pub id: u64,
    /// Status prior to the change.
    pub previous: VaultStatus,
    /// Status after the change.
    pub current: VaultStatus,
}
