//! The `VaultRegistry` contract: an authenticated on-chain index of deployed
//! Cadence vaults.
//!
//! ## Responsibility
//!
//! The registry does exactly one thing: it maintains a dense `u64 -> VaultRecord`
//! index of deployed vaults, plus a secondary `treasury -> [id]` index, and lets
//! authorised writers register vaults and advance their lifecycle status. It never
//! moves funds.
//!
//! ## Authorisation
//!
//! Writes (`register`, `set_status`) are gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module: the
//! caller MUST hold [`roles::FACTORY_ADMIN`](cadence_access_control::roles::FACTORY_ADMIN),
//! the role designated for accounts that create vaults via the factory and register
//! them. The deployer is bootstrapped as `ROOT_ADMIN` (so it can grant the writer
//! role) and as a `FACTORY_ADMIN` itself. Reads are open.
//!
//! ## Cross-contract surface
//!
//! [`VaultRegistration`] is the `#[odra::external_contract]` trait the factory calls
//! to register a freshly deployed vault without depending on the concrete registry
//! type — it builds a `VaultRegistrationContractRef::new(env, registry_addr)` from a
//! resolved `Address`.

use crate::errors::Error;
use crate::events::{VaultRegistered, VaultStatusChanged};
use crate::types::{VaultRecord, VaultStatus};
use cadence_access_control::roles;
use cadence_access_control::AccessControl;
use odra::prelude::*;

/// The cross-contract registration interface the factory calls.
///
/// Declared as an `#[odra::external_contract]` so a caller holding only the
/// registry's `Address` can register a vault via
/// `VaultRegistrationContractRef::new(env, addr).register(...)`.
#[odra::external_contract]
pub trait VaultRegistration {
    /// Index a freshly deployed vault and return its assigned registry id.
    ///
    /// Caller MUST hold [`roles::FACTORY_ADMIN`]. Reverts
    /// [`Error::AlreadyRegistered`] if `vault` is already indexed.
    fn register(&mut self, vault: Address, treasury: Address, mandate_hash: [u8; 32]) -> u64;
}

/// On-chain registry of deployed Cadence vaults.
#[odra::module(
    events = [VaultRegistered, VaultStatusChanged],
    errors = Error
)]
pub struct VaultRegistry {
    /// Shared RBAC sub-module gating all writes.
    ac: SubModule<AccessControl>,
    /// Dense id counter; also the count of registered vaults.
    count: Var<u64>,
    /// Primary index: registry id -> record.
    vaults: Mapping<u64, VaultRecord>,
    /// Reverse lookup: vault address -> assigned id (also serves as a
    /// uniqueness guard so the same vault cannot be registered twice).
    vault_ids: Mapping<Address, Option<u64>>,
    /// Secondary index: treasury -> list of registry ids it owns.
    by_treasury_index: Mapping<Address, Vec<u64>>,
}

#[odra::module]
impl VaultRegistry {
    /// Bootstrap: the deployer becomes `ROOT_ADMIN` (so it can grant writer roles)
    /// and is granted [`roles::FACTORY_ADMIN`] so it can register vaults directly.
    pub fn init(&mut self) {
        let deployer = self.env().caller();
        self.count.set(0);
        self.ac.grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
        self.ac.grant_unchecked(roles::FACTORY_ADMIN, deployer, deployer);
    }

    /// Index a freshly deployed vault and return its assigned registry id.
    ///
    /// Caller MUST hold [`roles::FACTORY_ADMIN`]. Reverts
    /// [`Error::AlreadyRegistered`] if `vault` is already indexed and
    /// [`Error::Overflow`] if the id counter would wrap.
    pub fn register(
        &mut self,
        vault: Address,
        treasury: Address,
        mandate_hash: [u8; 32],
    ) -> u64 {
        self.assert_writer();
        if self.vault_ids.get(&vault).flatten().is_some() {
            self.env().revert(Error::AlreadyRegistered);
        }

        let id = self.count.get_or_default();
        let next = match id.checked_add(1) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };

        let record = VaultRecord {
            id,
            vault,
            treasury,
            mandate_hash,
            registered_at: self.env().get_block_time(),
            status: VaultStatus::Active,
        };

        self.vaults.set(&id, record);
        self.vault_ids.set(&vault, Some(id));
        self.count.set(next);

        let mut owned = self.by_treasury_index.get_or_default(&treasury);
        owned.push(id);
        self.by_treasury_index.set(&treasury, owned);

        self.env().emit_event(VaultRegistered { id, vault, treasury, mandate_hash });
        id
    }

    /// Advance a vault's lifecycle status.
    ///
    /// Caller MUST hold [`roles::FACTORY_ADMIN`]. Reverts [`Error::UnknownVault`]
    /// for an unknown id and [`Error::InvalidStatusTransition`] if the move is not
    /// permitted by [`VaultStatus::can_transition_to`].
    pub fn set_status(&mut self, id: u64, status: VaultStatus) {
        self.assert_writer();
        let record = self
            .vaults
            .get(&id)
            .unwrap_or_revert_with(&self.env(), Error::UnknownVault);

        if !record.status.can_transition_to(&status) {
            self.env().revert(Error::InvalidStatusTransition);
        }

        let previous = record.status;
        let updated = VaultRecord { status: status.clone(), ..record };
        self.vaults.set(&id, updated);
        self.env().emit_event(VaultStatusChanged { id, previous, current: status });
    }

    /// Fetch a record by registry id, or `None` if unknown.
    pub fn get(&self, id: u64) -> Option<VaultRecord> {
        self.vaults.get(&id)
    }

    /// Total number of registered vaults (also the next id to be assigned).
    pub fn count(&self) -> u64 {
        self.count.get_or_default()
    }

    /// Paginated enumeration: records for ids in `[start, start + limit)` that
    /// exist. Out-of-range or missing ids are skipped, so the returned `Vec` may
    /// be shorter than `limit`.
    pub fn enumerate(&self, start: u64, limit: u64) -> Vec<VaultRecord> {
        let mut out: Vec<VaultRecord> = Vec::new();
        let total = self.count.get_or_default();
        let end = match start.checked_add(limit) {
            Some(v) => v.min(total),
            None => total,
        };
        let mut id = start;
        while id < end {
            if let Some(record) = self.vaults.get(&id) {
                out.push(record);
            }
            id += 1;
        }
        out
    }

    /// All registry ids owned by `treasury`, in registration order.
    pub fn by_treasury(&self, treasury: Address) -> Vec<u64> {
        self.by_treasury_index.get_or_default(&treasury)
    }

    /// Whether `who` holds the registry writer role ([`roles::FACTORY_ADMIN`]).
    pub fn is_writer(&self, who: Address) -> bool {
        self.ac.has_role(roles::FACTORY_ADMIN, who)
    }

    /// Grant the writer role ([`roles::FACTORY_ADMIN`]) to `who`. Caller MUST
    /// administer that role (the deployer holds `ROOT_ADMIN`, its default admin).
    pub fn grant_writer(&mut self, who: Address) {
        self.ac.grant_role(roles::FACTORY_ADMIN, who);
    }

    /// Revoke the writer role from `who`. Caller MUST administer the role.
    pub fn revoke_writer(&mut self, who: Address) {
        self.ac.revoke_role(roles::FACTORY_ADMIN, who);
    }

    // ----- internal helpers (never exposed as entrypoints) -----

    /// Revert [`Error::Unauthorized`] unless the caller holds the writer role.
    fn assert_writer(&self) {
        let caller = self.env().caller();
        if !self.ac.has_role(roles::FACTORY_ADMIN, caller) {
            self.env().revert(Error::Unauthorized);
        }
    }
}
