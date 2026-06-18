//! # Cadence Vault Factory
//!
//! The factory is the single authorised entry point for bringing a new Cadence
//! execution vault into existence and indexing it in the
//! [`VaultRegistry`](cadence_vault_registry::registry::VaultRegistry).
//!
//! ## Why "record intent + emit init args", not on-chain instantiation
//!
//! Ethereum's `CREATE2` lets a factory contract deterministically instantiate a
//! child contract from on-chain bytecode in the same transaction. **Casper has no
//! equivalent**, and Odra 2.8.1 provides no host API for a running contract to
//! install stored wasm and obtain a fresh contract `Address` on-chain. A contract
//! therefore cannot deploy another contract.
//!
//! Rather than pretend otherwise, this factory implements the **registry-driven
//! "record intent + emit canonical init args" model**:
//!
//! 1. [`create_vault`](factory::VaultFactory::create_vault) validates the request,
//!    assigns a dense intent id, and stores a [`VaultIntent`](storage::VaultIntent)
//!    so the request is auditable on-chain.
//! 2. It registers the *target* vault address in the registry via the
//!    [`VaultRegistration`](cadence_vault_registry::registry::VaultRegistration)
//!    cross-contract trait, so off-chain tooling and the guardian can enumerate it
//!    immediately.
//! 3. It emits the canonical [`VaultDeployed`](events::VaultDeployed) /
//!    [`VaultIntentRecorded`](events::VaultIntentRecorded) events carrying the
//!    init-arg payload an off-chain deploy script consumes to install the vault
//!    wasm at the committed address.
//!
//! The caller supplies the target vault `Address` (computed off-chain from the
//! known vault package-hash + the caller's deploy nonce). The factory is the
//! source of truth for *which* vaults are sanctioned, registered, and what init
//! args they MUST be deployed with — it is not the literal installer.
//!
//! ## Authorisation
//!
//! All state-changing entrypoints are gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module and
//! require [`FACTORY_ADMIN`](cadence_access_control::roles::FACTORY_ADMIN). The
//! deployer is bootstrapped as `ROOT_ADMIN` (so it can delegate the role) and as a
//! `FACTORY_ADMIN` itself. To register vaults the factory's own address must in
//! turn hold `FACTORY_ADMIN` on the registry (granted by the registry deployer via
//! `grant_writer`).
//!
//! ## Layout
//!
//! * [`errors`] — the [`Error`](errors::Error) code set.
//! * [`storage`] — the [`VaultIntent`](storage::VaultIntent) record and the
//!   [`VaultFactory`](storage::VaultFactory) storage layout.
//! * [`events`] — [`VaultDeployed`](events::VaultDeployed),
//!   [`VaultIntentRecorded`](events::VaultIntentRecorded) and
//!   [`WasmUpdated`](events::WasmUpdated).
//! * [`factory`] — the entrypoints (`create_vault`, config, RBAC, views).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod errors;
pub mod events;
pub mod factory;
pub mod storage;

pub use storage::VaultFactory;
