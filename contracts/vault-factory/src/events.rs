//! Events emitted by the [`VaultFactory`](crate::factory::VaultFactory).
//!
//! The factory does not install wasm on-chain (Casper has no `CREATE2`; see the
//! crate-level docs). Instead these events carry the canonical init-arg payload an
//! off-chain deploy script consumes to install the vault at the committed address.

use odra::casper_types::bytesrepr::Bytes;
use odra::prelude::*;

/// Emitted when a vault creation intent is recorded and indexed in the registry.
///
/// This is the auditable on-chain record that the factory sanctioned a vault: the
/// intent has been assigned an `intent_id`, persisted, and registered. A deploy
/// script keys off this event together with [`VaultDeployed`].
#[odra::event]
pub struct VaultIntentRecorded {
    /// Dense factory intent id assigned to this request.
    pub intent_id: u64,
    /// The committed target address the vault wasm MUST be installed at.
    pub vault: Address,
    /// The treasury that will own / fund the vault.
    pub treasury: Address,
    /// The mandate hash the vault will execute under.
    pub mandate_hash: [u8; 32],
}

/// Emitted with the canonical init args an off-chain script uses to deploy the vault.
///
/// Separate from [`VaultIntentRecorded`] so the init-arg payload (which a deploy
/// script consumes) is a distinct, stable topic from the lifecycle/registry signal.
#[odra::event]
pub struct VaultDeployed {
    /// Factory intent id linking this payload to its [`VaultIntentRecorded`].
    pub intent_id: u64,
    /// The committed target vault address.
    pub vault: Address,
    /// The vault package-hash reference (stored wasm) to install at `vault`.
    pub wasm_ref: Bytes,
    /// Canonical init arg: the treasury that owns / signs for the vault.
    pub treasury: Address,
    /// Canonical init arg: the agent authorised to execute slices.
    pub agent: Address,
    /// Canonical init arg: the mandate hash the vault executes under.
    pub mandate_hash: [u8; 32],
    /// Canonical init arg: the registry the vault reports lifecycle to.
    pub registry: Address,
}

/// Emitted when the configured vault package-hash (stored wasm reference) changes.
#[odra::event]
pub struct WasmUpdated {
    /// The previous package-hash reference, if any.
    pub previous: Option<Bytes>,
    /// The new package-hash reference.
    pub current: Bytes,
}
