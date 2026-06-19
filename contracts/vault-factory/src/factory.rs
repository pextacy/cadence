//! The [`VaultFactory`] entrypoints: record a sanctioned vault-creation intent,
//! register it in the registry, and emit the canonical init-arg payload an
//! off-chain script deploys with.
//!
//! See the crate-level docs for why this is a "record intent + emit init args"
//! factory and not an on-chain instantiator: Casper has no `CREATE2` and Odra 2.8.1
//! exposes no host API for a contract to install stored wasm on-chain.

use crate::errors::Error;
use crate::events::{VaultDeployed, VaultIntentRecorded, WasmUpdated};
use crate::storage::{VaultFactory, VaultIntent};
use cadence_access_control::roles;
use cadence_vault_registry::registry::VaultRegistrationContractRef;
use odra::casper_types::bytesrepr::Bytes;
use odra::prelude::*;
use odra::ContractRef;

#[odra::module]
impl VaultFactory {
    /// Bootstrap the factory.
    ///
    /// `registry` is the address of the deployed
    /// [`VaultRegistry`](cadence_vault_registry::registry::VaultRegistry) this
    /// factory registers vaults in; `vault_wasm_ref` is the vault package-hash
    /// (stored wasm reference) new vaults are deployed from. The deployer becomes
    /// `ROOT_ADMIN` (so it can delegate) and a `FACTORY_ADMIN` (so it can create
    /// vaults directly). Reverts [`Error::EmptyWasmRef`] on an empty wasm ref.
    pub fn init(&mut self, registry: Address, vault_wasm_ref: Bytes) {
        if vault_wasm_ref.is_empty() {
            self.env().revert(Error::EmptyWasmRef);
        }
        let deployer = self.env().caller();
        self.count.set(0);
        self.registry.set(registry);
        self.vault_wasm_ref.set(vault_wasm_ref);
        self.ac
            .grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
        self.ac
            .grant_unchecked(roles::FACTORY_ADMIN, deployer, deployer);
    }

    /// Sanction a new vault: record the intent, register it, emit init args.
    ///
    /// Caller MUST hold [`roles::FACTORY_ADMIN`]. `vault` is the committed target
    /// address (computed off-chain) the vault wasm will be installed at; `treasury`
    /// owns/funds it; `agent` executes slices; `mandate_hash` is the mandate it runs
    /// under. Returns the assigned factory intent id.
    ///
    /// Steps:
    /// 1. validate inputs (non-equal, distinct addresses; wasm configured),
    /// 2. record an immutable [`VaultIntent`],
    /// 3. register `vault` in the registry via [`VaultRegistrationContractRef`],
    /// 4. emit [`VaultIntentRecorded`] + [`VaultDeployed`] (the init-arg payload).
    ///
    /// Reverts [`Error::Unauthorized`], [`Error::InvalidInput`],
    /// [`Error::WasmNotConfigured`] or [`Error::Overflow`].
    pub fn create_vault(
        &mut self,
        vault: Address,
        treasury: Address,
        agent: Address,
        mandate_hash: [u8; 32],
    ) -> u64 {
        self.assert_admin();
        self.validate_create(vault, treasury, agent);

        let wasm_ref = self
            .vault_wasm_ref
            .get()
            .unwrap_or_revert_with(&self.env(), Error::WasmNotConfigured);
        let registry = self
            .registry
            .get()
            .unwrap_or_revert_with(&self.env(), Error::WasmNotConfigured);

        let id = self.count.get_or_default();
        let next = match id.checked_add(1) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };

        let intent = VaultIntent {
            id,
            vault,
            treasury,
            agent,
            mandate_hash,
            wasm_ref: wasm_ref.clone(),
            recorded_at: self.env().get_block_time(),
        };
        self.intents.set(&id, intent);
        self.count.set(next);

        // Index the vault in the registry (cross-contract). The factory's own
        // address must hold FACTORY_ADMIN on the registry for this to succeed.
        VaultRegistrationContractRef::new(self.env(), registry).register(
            vault,
            treasury,
            mandate_hash,
        );

        self.env().emit_event(VaultIntentRecorded {
            intent_id: id,
            vault,
            treasury,
            mandate_hash,
        });
        self.env().emit_event(VaultDeployed {
            intent_id: id,
            vault,
            wasm_ref,
            treasury,
            agent,
            mandate_hash,
            registry,
        });
        id
    }

    /// Update the configured vault package-hash (stored wasm reference) future
    /// vaults are deployed from. Caller MUST hold [`roles::FACTORY_ADMIN`]. Reverts
    /// [`Error::EmptyWasmRef`] on an empty ref. Emits [`WasmUpdated`].
    pub fn set_vault_wasm(&mut self, wasm_ref: Bytes) {
        self.assert_admin();
        if wasm_ref.is_empty() {
            self.env().revert(Error::EmptyWasmRef);
        }
        let previous = self.vault_wasm_ref.get();
        self.vault_wasm_ref.set(wasm_ref.clone());
        self.env().emit_event(WasmUpdated {
            previous,
            current: wasm_ref,
        });
    }

    // ----- views -----

    /// The registry address vaults are registered in.
    pub fn registry(&self) -> Option<Address> {
        self.registry.get()
    }

    /// The configured vault package-hash (stored wasm reference), if any.
    pub fn vault_wasm(&self) -> Option<Bytes> {
        self.vault_wasm_ref.get()
    }

    /// Fetch a recorded intent by id, or `None` if unknown.
    pub fn get_intent(&self, id: u64) -> Option<VaultIntent> {
        self.intents.get(&id)
    }

    /// Total number of recorded intents (also the next id to be assigned).
    pub fn count(&self) -> u64 {
        self.count.get_or_default()
    }

    // ----- RBAC management (mirrors the registry's surface) -----

    /// Whether `who` holds the factory admin role ([`roles::FACTORY_ADMIN`]).
    pub fn is_admin(&self, who: Address) -> bool {
        self.ac.has_role(roles::FACTORY_ADMIN, who)
    }

    /// Grant [`roles::FACTORY_ADMIN`] to `who`. Caller MUST administer the role
    /// (the deployer holds `ROOT_ADMIN`, its default admin).
    pub fn grant_admin(&mut self, who: Address) {
        self.ac.grant_role(roles::FACTORY_ADMIN, who);
    }

    /// Revoke [`roles::FACTORY_ADMIN`] from `who`. Caller MUST administer the role.
    pub fn revoke_admin(&mut self, who: Address) {
        self.ac.revoke_role(roles::FACTORY_ADMIN, who);
    }

    // ----- internal helpers (never exposed as entrypoints) -----

    /// Revert [`Error::Unauthorized`] unless the caller holds `FACTORY_ADMIN`.
    fn assert_admin(&self) {
        let caller = self.env().caller();
        if !self.ac.has_role(roles::FACTORY_ADMIN, caller) {
            self.env().revert(Error::Unauthorized);
        }
    }

    /// Validate `create_vault` inputs: the three addresses must be pairwise
    /// distinct (a vault cannot be its own treasury or agent, and the treasury and
    /// agent must differ). Reverts [`Error::InvalidInput`] otherwise.
    fn validate_create(&self, vault: Address, treasury: Address, agent: Address) {
        if vault == treasury || vault == agent || treasury == agent {
            self.env().revert(Error::InvalidInput);
        }
    }
}
