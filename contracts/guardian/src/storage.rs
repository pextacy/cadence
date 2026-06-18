//! Cross-contract surface the [`Guardian`](crate::guardian::Guardian) calls.
//!
//! The guardian owns only an `Address` for the vault registry and for each vault
//! it pauses. It depends on neither concrete type at the call site — it builds a
//! `…ContractRef::new(self.env(), addr)` from the resolved address and calls
//! through these `#[odra::external_contract]` traits.
//!
//! * [`VaultRegistryView`] mirrors the read entrypoints of
//!   [`cadence_vault_registry::registry::VaultRegistry`] the guardian needs to
//!   enumerate the active vault set. The method signatures MUST match the
//!   registry's entrypoints byte-for-byte so the dynamic dispatch resolves.
//! * [`VaultControl`] is the pause/resume interface every Cadence vault exposes
//!   to its guardian. `pause` MUST be idempotent (a no-op, not a revert, when the
//!   vault is already paused) so a desk-wide sweep tolerates vaults that an
//!   earlier batch — or the vault's own agent — already paused.

use cadence_vault_registry::types::VaultRecord;
use odra::prelude::*;

/// Read-only view of the vault registry the guardian enumerates over.
///
/// Signatures mirror [`cadence_vault_registry::registry::VaultRegistry`] so a
/// `VaultRegistryViewContractRef` dispatches to the live registry.
#[odra::external_contract]
pub trait VaultRegistryView {
    /// Total number of registered vaults (also the next id to be assigned).
    fn count(&self) -> u64;

    /// Paginated enumeration: records for ids in `[start, start + limit)` that
    /// exist. Missing ids are skipped, so the returned `Vec` may be shorter than
    /// `limit`.
    fn enumerate(&self, start: u64, limit: u64) -> Vec<VaultRecord>;
}

/// The pause/resume control surface every Cadence vault exposes to its guardian.
///
/// `pause` MUST be idempotent: when the vault is already paused it returns
/// normally rather than reverting, so a desk-wide fan-out never aborts mid-sweep
/// on an already-paused vault. `resume` is likewise idempotent for the resume
/// path.
#[odra::external_contract]
pub trait VaultControl {
    /// Engage the vault's circuit-breaker. Idempotent: a no-op if already paused.
    fn pause(&mut self);

    /// Lift the vault's circuit-breaker. Idempotent: a no-op if already active.
    fn resume(&mut self);
}
