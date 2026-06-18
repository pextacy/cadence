//! Integration tests for the `VaultFactory` contract.
//!
//! The factory records a sanctioned vault-creation intent, registers the target
//! vault in a real `VaultRegistry`, and emits the canonical init-arg payload. These
//! tests deploy both contracts and grant the factory's address the registry writer
//! role so the cross-contract `register` call succeeds.

use cadence_vault_factory::errors::Error;
use cadence_vault_factory::factory::{VaultFactoryHostRef, VaultFactoryInitArgs};
use cadence_vault_factory::storage::VaultFactory;
use cadence_vault_registry::registry::VaultRegistry;
use odra::casper_types::bytesrepr::Bytes;
use odra::host::{Deployer, HostEnv, NoArgs};
use odra::prelude::{Address, Addressable};

struct Fixture {
    env: HostEnv,
    factory: VaultFactoryHostRef,
    registry: cadence_vault_registry::registry::VaultRegistryHostRef,
    admin: Address,
    treasury: Address,
    agent: Address,
    vault_1: Address,
    vault_2: Address,
    outsider: Address,
}

fn wasm_ref() -> Bytes {
    // Stand-in for a vault package-hash reference; only its non-emptiness matters.
    Bytes::from(vec![0xCAu8, 0xDE, 0x57, 0x01])
}

fn mandate(tag: u8) -> [u8; 32] {
    let mut m = [0u8; 32];
    m[0] = tag;
    m
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let admin = env.get_account(0);
    let treasury = env.get_account(1);
    let agent = env.get_account(2);
    let vault_1 = env.get_account(3);
    let vault_2 = env.get_account(4);
    let outsider = env.get_account(5);

    env.set_caller(admin);
    let registry = VaultRegistry::deploy(&env, NoArgs);
    let factory = VaultFactory::deploy(
        &env,
        VaultFactoryInitArgs {
            registry: registry.address(),
            vault_wasm_ref: wasm_ref(),
        },
    );
    // The factory must hold the registry writer role to register vaults.
    let mut registry = registry;
    registry.grant_writer(factory.address());

    Fixture {
        env,
        factory,
        registry,
        admin,
        treasury,
        agent,
        vault_1,
        vault_2,
        outsider,
    }
}

#[test]
fn deployer_is_admin_and_config_is_set() {
    let fx = setup();
    assert!(fx.factory.is_admin(fx.admin));
    assert!(!fx.factory.is_admin(fx.outsider));
    assert_eq!(fx.factory.count(), 0);
    assert_eq!(fx.factory.registry(), Some(fx.registry.address()));
    assert_eq!(fx.factory.vault_wasm(), Some(wasm_ref()));
}

#[test]
fn empty_wasm_ref_at_init_reverts() {
    let env = odra_test::env();
    let admin = env.get_account(0);
    env.set_caller(admin);
    let registry = VaultRegistry::deploy(&env, NoArgs);
    let result = VaultFactory::try_deploy(
        &env,
        VaultFactoryInitArgs {
            registry: registry.address(),
            vault_wasm_ref: Bytes::from(Vec::<u8>::new()),
        },
    );
    assert_eq!(result.err(), Some(Error::EmptyWasmRef.into()));
}

#[test]
fn create_vault_records_intent() {
    let mut fx = setup();
    let id = fx
        .factory
        .create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1));
    assert_eq!(id, 0);
    assert_eq!(fx.factory.count(), 1);

    let intent = fx.factory.get_intent(0).expect("intent exists");
    assert_eq!(intent.id, 0);
    assert_eq!(intent.vault, fx.vault_1);
    assert_eq!(intent.treasury, fx.treasury);
    assert_eq!(intent.agent, fx.agent);
    assert_eq!(intent.mandate_hash, mandate(1));
    assert_eq!(intent.wasm_ref, wasm_ref());
}

#[test]
fn create_vault_registers_in_registry() {
    let mut fx = setup();
    fx.factory
        .create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(7));

    // The registry now indexes the target vault.
    assert_eq!(fx.registry.count(), 1);
    let record = fx.registry.get(0).expect("registry record exists");
    assert_eq!(record.vault, fx.vault_1);
    assert_eq!(record.treasury, fx.treasury);
    assert_eq!(record.mandate_hash, mandate(7));
}

#[test]
fn intent_ids_are_dense_and_monotonic() {
    let mut fx = setup();
    let id0 = fx
        .factory
        .create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1));
    let id1 = fx
        .factory
        .create_vault(fx.vault_2, fx.treasury, fx.agent, mandate(2));
    assert_eq!((id0, id1), (0, 1));
    assert_eq!(fx.factory.count(), 2);
    assert_eq!(fx.registry.count(), 2);
}

#[test]
fn non_admin_cannot_create_vault() {
    let mut fx = setup();
    fx.env.set_caller(fx.outsider);
    let err = fx
        .factory
        .try_create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1))
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn granted_admin_can_create_vault() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.factory.grant_admin(fx.outsider);
    assert!(fx.factory.is_admin(fx.outsider));

    fx.env.set_caller(fx.outsider);
    let id = fx
        .factory
        .create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1));
    assert_eq!(id, 0);
}

#[test]
fn revoked_admin_cannot_create_vault() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.factory.grant_admin(fx.outsider);
    fx.factory.revoke_admin(fx.outsider);
    assert!(!fx.factory.is_admin(fx.outsider));

    fx.env.set_caller(fx.outsider);
    let err = fx
        .factory
        .try_create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1))
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn duplicate_addresses_are_rejected() {
    let mut fx = setup();
    // vault == treasury
    let err = fx
        .factory
        .try_create_vault(fx.vault_1, fx.vault_1, fx.agent, mandate(1))
        .unwrap_err();
    assert_eq!(err, Error::InvalidInput.into());

    // treasury == agent
    let err = fx
        .factory
        .try_create_vault(fx.vault_1, fx.treasury, fx.treasury, mandate(1))
        .unwrap_err();
    assert_eq!(err, Error::InvalidInput.into());
}

#[test]
fn set_vault_wasm_updates_config() {
    let mut fx = setup();
    let new_ref = Bytes::from(vec![0x01u8, 0x02, 0x03]);
    fx.factory.set_vault_wasm(new_ref.clone());
    assert_eq!(fx.factory.vault_wasm(), Some(new_ref));
}

#[test]
fn set_empty_vault_wasm_reverts() {
    let mut fx = setup();
    let err = fx
        .factory
        .try_set_vault_wasm(Bytes::from(Vec::<u8>::new()))
        .unwrap_err();
    assert_eq!(err, Error::EmptyWasmRef.into());
}

#[test]
fn non_admin_cannot_set_vault_wasm() {
    let mut fx = setup();
    fx.env.set_caller(fx.outsider);
    let err = fx
        .factory
        .try_set_vault_wasm(Bytes::from(vec![0x09u8]))
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn get_unknown_intent_returns_none() {
    let fx = setup();
    assert!(fx.factory.get_intent(0).is_none());
}

#[test]
fn new_wasm_ref_applies_to_subsequent_intents() {
    let mut fx = setup();
    fx.factory
        .create_vault(fx.vault_1, fx.treasury, fx.agent, mandate(1));
    let new_ref = Bytes::from(vec![0xAAu8, 0xBB]);
    fx.factory.set_vault_wasm(new_ref.clone());
    fx.factory
        .create_vault(fx.vault_2, fx.treasury, fx.agent, mandate(2));

    assert_eq!(fx.factory.get_intent(0).unwrap().wasm_ref, wasm_ref());
    assert_eq!(fx.factory.get_intent(1).unwrap().wasm_ref, new_ref);
}
