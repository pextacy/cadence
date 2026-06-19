//! Persistent state for the [`VaultFactory`](crate::factory::VaultFactory): the
//! per-request intent record and the module's storage layout.

#[allow(unused_imports)]
use crate::errors::Error;
use crate::events::{VaultDeployed, VaultIntentRecorded, WasmUpdated};
use cadence_access_control::AccessControl;
use odra::casper_types::bytesrepr::Bytes;
use odra::prelude::*;

/// One recorded vault-creation intent.
///
/// Every field is set once at [`create_vault`](crate::factory::VaultFactory::create_vault)
/// time and never mutated — the intent is an immutable, auditable record of what the
/// factory sanctioned and the init args an off-chain script MUST deploy with.
#[odra::odra_type]
pub struct VaultIntent {
    /// Dense factory intent id (the `Mapping` key).
    pub id: u64,
    /// The committed target address the vault wasm is installed at.
    pub vault: Address,
    /// The treasury that owns / funds the vault.
    pub treasury: Address,
    /// The agent authorised to execute slices under the mandate.
    pub agent: Address,
    /// The 32-byte mandate hash the vault executes under.
    pub mandate_hash: [u8; 32],
    /// The vault package-hash reference (stored wasm) at intent time.
    pub wasm_ref: Bytes,
    /// Block-time (ms since epoch) the intent was recorded.
    pub recorded_at: u64,
}

/// Factory for Cadence execution vaults.
///
/// Stores the vault package-hash to deploy, the registry it reports to, the shared
/// RBAC sub-module, and a dense index of recorded intents. See the crate-level docs
/// for why this records intent + emits init args rather than instantiating on-chain.
#[odra::module(
    events = [VaultIntentRecorded, VaultDeployed, WasmUpdated],
    errors = Error
)]
pub struct VaultFactory {
    /// Shared RBAC sub-module gating all writes (`FACTORY_ADMIN`).
    pub(crate) ac: SubModule<AccessControl>,
    /// The vault package-hash reference (stored wasm) new vaults are deployed from.
    pub(crate) vault_wasm_ref: Var<Bytes>,
    /// The registry address freshly created vaults are registered in.
    pub(crate) registry: Var<Address>,
    /// Dense intent id counter; also the count of recorded intents.
    pub(crate) count: Var<u64>,
    /// Primary index: intent id -> record.
    pub(crate) intents: Mapping<u64, VaultIntent>,
    /// Optional treasury multisig gate. When set, `create_vault` requires the
    /// action to have cleared an M-of-N multisig approval before it will run.
    pub(crate) multisig: Var<Address>,
}
