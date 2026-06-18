//! Integration tests for the `VaultRegistry` contract.

use cadence_vault_registry::registry::VaultRegistry;
use cadence_vault_registry::types::VaultStatus;
use odra::host::{Deployer, HostEnv, NoArgs};
use odra::prelude::{Address, Addressable};

struct Fixture {
    env: HostEnv,
    registry: cadence_vault_registry::registry::VaultRegistryHostRef,
    admin: Address,
    treasury_a: Address,
    treasury_b: Address,
    vault_1: Address,
    vault_2: Address,
    vault_3: Address,
    outsider: Address,
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let admin = env.get_account(0);
    let treasury_a = env.get_account(1);
    let treasury_b = env.get_account(2);
    // Reuse later accounts as stand-in vault addresses.
    let vault_1 = env.get_account(3);
    let vault_2 = env.get_account(4);
    let vault_3 = env.get_account(5);
    let outsider = env.get_account(6);
    env.set_caller(admin);
    let registry = VaultRegistry::deploy(&env, NoArgs);
    Fixture {
        env,
        registry,
        admin,
        treasury_a,
        treasury_b,
        vault_1,
        vault_2,
        vault_3,
        outsider,
    }
}

fn mandate(tag: u8) -> [u8; 32] {
    let mut m = [0u8; 32];
    m[0] = tag;
    m
}

#[test]
fn deployer_is_a_writer() {
    let fx = setup();
    assert!(fx.registry.is_writer(fx.admin));
    assert!(!fx.registry.is_writer(fx.outsider));
    assert_eq!(fx.registry.count(), 0);
}

#[test]
fn register_indexes_a_vault() {
    let mut fx = setup();
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    assert_eq!(id, 0);
    assert_eq!(fx.registry.count(), 1);

    let record = fx.registry.get(0).expect("record exists");
    assert_eq!(record.id, 0);
    assert_eq!(record.vault, fx.vault_1);
    assert_eq!(record.treasury, fx.treasury_a);
    assert_eq!(record.mandate_hash, mandate(1));
    assert_eq!(record.status, VaultStatus::Active);
}

#[test]
fn ids_are_dense_and_monotonic() {
    let mut fx = setup();
    let id0 = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    let id1 = fx.registry.register(fx.vault_2, fx.treasury_a, mandate(2));
    let id2 = fx.registry.register(fx.vault_3, fx.treasury_b, mandate(3));
    assert_eq!((id0, id1, id2), (0, 1, 2));
    assert_eq!(fx.registry.count(), 3);
}

#[test]
fn duplicate_vault_is_rejected() {
    let mut fx = setup();
    fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    let err = fx
        .registry
        .try_register(fx.vault_1, fx.treasury_b, mandate(9))
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::AlreadyRegistered.into()
    );
}

#[test]
fn non_writer_cannot_register() {
    let mut fx = setup();
    fx.env.set_caller(fx.outsider);
    let err = fx
        .registry
        .try_register(fx.vault_1, fx.treasury_a, mandate(1))
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::Unauthorized.into()
    );
}

#[test]
fn granted_writer_can_register() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.registry.grant_writer(fx.outsider);
    assert!(fx.registry.is_writer(fx.outsider));

    fx.env.set_caller(fx.outsider);
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    assert_eq!(id, 0);
}

#[test]
fn revoked_writer_cannot_register() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.registry.grant_writer(fx.outsider);
    fx.registry.revoke_writer(fx.outsider);
    assert!(!fx.registry.is_writer(fx.outsider));

    fx.env.set_caller(fx.outsider);
    let err = fx
        .registry
        .try_register(fx.vault_1, fx.treasury_a, mandate(1))
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::Unauthorized.into()
    );
}

#[test]
fn by_treasury_groups_records() {
    let mut fx = setup();
    fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    fx.registry.register(fx.vault_2, fx.treasury_b, mandate(2));
    fx.registry.register(fx.vault_3, fx.treasury_a, mandate(3));

    assert_eq!(fx.registry.by_treasury(fx.treasury_a), vec![0, 2]);
    assert_eq!(fx.registry.by_treasury(fx.treasury_b), vec![1]);
    assert!(fx.registry.by_treasury(fx.outsider).is_empty());
}

#[test]
fn enumerate_is_paginated() {
    let mut fx = setup();
    fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    fx.registry.register(fx.vault_2, fx.treasury_a, mandate(2));
    fx.registry.register(fx.vault_3, fx.treasury_b, mandate(3));

    let page = fx.registry.enumerate(0, 2);
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].id, 0);
    assert_eq!(page[1].id, 1);

    let tail = fx.registry.enumerate(2, 10);
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].id, 2);

    // Out-of-range start yields nothing.
    assert!(fx.registry.enumerate(100, 5).is_empty());
}

#[test]
fn set_status_advances_lifecycle() {
    let mut fx = setup();
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    fx.registry.set_status(id, VaultStatus::Paused);
    assert_eq!(fx.registry.get(id).unwrap().status, VaultStatus::Paused);

    fx.registry.set_status(id, VaultStatus::Active);
    assert_eq!(fx.registry.get(id).unwrap().status, VaultStatus::Active);

    fx.registry.set_status(id, VaultStatus::Closed);
    assert_eq!(fx.registry.get(id).unwrap().status, VaultStatus::Closed);
}

#[test]
fn closed_is_terminal() {
    let mut fx = setup();
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    fx.registry.set_status(id, VaultStatus::Closed);
    let err = fx
        .registry
        .try_set_status(id, VaultStatus::Active)
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::InvalidStatusTransition.into()
    );
}

#[test]
fn no_op_transition_is_rejected() {
    let mut fx = setup();
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    let err = fx
        .registry
        .try_set_status(id, VaultStatus::Active)
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::InvalidStatusTransition.into()
    );
}

#[test]
fn set_status_on_unknown_vault_reverts() {
    let mut fx = setup();
    let err = fx
        .registry
        .try_set_status(42, VaultStatus::Paused)
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::UnknownVault.into()
    );
}

#[test]
fn non_writer_cannot_set_status() {
    let mut fx = setup();
    let id = fx.registry.register(fx.vault_1, fx.treasury_a, mandate(1));
    fx.env.set_caller(fx.outsider);
    let err = fx
        .registry
        .try_set_status(id, VaultStatus::Paused)
        .unwrap_err();
    assert_eq!(
        err,
        cadence_vault_registry::errors::Error::Unauthorized.into()
    );
}

#[test]
fn get_unknown_returns_none() {
    let fx = setup();
    assert!(fx.registry.get(0).is_none());
}

// Assert the `#[odra::external_contract]` surface generated its cross-contract
// `ContractRef` (used on-chain by the factory). Constructing one requires a
// `ContractEnv`, which only exists inside a contract, so here we only assert the
// type resolves and is sized — the registration logic itself is covered above.
#[test]
fn external_registration_ref_type_exists() {
    use cadence_vault_registry::registry::VaultRegistrationContractRef;
    fn assert_sized<T: Sized>() {}
    assert_sized::<VaultRegistrationContractRef>();
    let fx = setup();
    let addr = fx.registry.address();
    // The host can still resolve the deployed registry's address.
    assert_eq!(addr, fx.registry.address());
}
