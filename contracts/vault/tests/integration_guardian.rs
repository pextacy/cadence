//! Wave 3 integration: the desk-wide Guardian pauses REAL `ExecutionVault`s (not
//! the guardian crate's `MockVault`), proving the `VaultControl` cross-contract
//! surface and the GUARDIAN-role authorization line up end to end:
//! registry-driven fan-out → cross-contract `pause()` → vault accepts it because
//! the treasury wired the GUARDIAN role to the guardian contract.

mod common;

use odra::casper_types::U512;
use odra::host::{Deployer, HostRef, NoArgs};
use odra::prelude::Address;

use cadence_guardian::guardian::{Guardian, GuardianInitArgs};
use cadence_vault::vault::Status;
use cadence_vault_registry::registry::VaultRegistry;

use common::*;

/// Happy-path deploy args: a correctly-signed mandate over the full total.
fn happy_args(treasury: Address) -> DeployArgs {
    DeployArgs {
        price_floor: U512::zero(),
        price_ceiling: U512::zero(),
        signer: treasury,
        supplied_pk_account: treasury,
        signed_total: U512::from(TOTAL_SELL),
        install_total: U512::from(TOTAL_SELL),
        override_signature: None,
    }
}

#[test]
fn guardian_pauses_real_vaults_desk_wide() {
    let env = odra_test::env();
    let treasury = env.get_account(0);

    // Registry first — the deployer (treasury) is the authorized writer.
    env.set_caller(treasury);
    let mut registry = VaultRegistry::deploy(&env, NoArgs);

    // Two real vaults, funded to Active.
    let mut vault_a = try_deploy(&env, happy_args(treasury)).expect("vault a deploys");
    let mut vault_b = try_deploy(&env, happy_args(treasury)).expect("vault b deploys");
    env.set_caller(treasury);
    vault_a.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault_b.with_tokens(U512::from(TOTAL_SELL)).fund();
    assert_eq!(vault_a.get_status(), Status::Active);
    assert_eq!(vault_b.get_status(), Status::Active);

    // Register both (status defaults to Active in the registry).
    env.set_caller(treasury);
    registry.register(vault_a.contract_address(), treasury, [1u8; 32]);
    registry.register(vault_b.contract_address(), treasury, [2u8; 32]);

    // Guardian over the registry; its deployer (treasury) holds GUARDIAN on it.
    let mut guardian = Guardian::deploy(
        &env,
        GuardianInitArgs {
            registry: registry.contract_address(),
        },
    );

    // Each vault wires the GUARDIAN role to the guardian contract so the
    // desk-wide kill switch is authorized to pause it.
    env.set_caller(treasury);
    vault_a.set_guardian(guardian.contract_address());
    vault_b.set_guardian(guardian.contract_address());
    assert!(vault_a.is_guardian(guardian.contract_address()));

    // Desk-wide pause: the guardian enumerates the registry and fans out a
    // cross-contract pause() to every Active vault.
    env.set_caller(treasury);
    let affected = guardian.global_pause(0, 10);

    assert_eq!(affected, 2, "both Active vaults must be paused");
    assert_eq!(vault_a.get_status(), Status::Paused);
    assert_eq!(vault_b.get_status(), Status::Paused);
}

#[test]
fn vault_rejects_pause_from_an_unwired_guardian() {
    // Sanity: the cross-contract pause only works because the role was wired. A
    // guardian the vault never granted GUARDIAN to cannot pause it.
    let env = odra_test::env();
    let treasury = env.get_account(0);

    env.set_caller(treasury);
    let mut registry = VaultRegistry::deploy(&env, NoArgs);
    let vault = try_deploy(&env, happy_args(treasury)).expect("vault deploys");
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    registry.register(vault.contract_address(), treasury, [9u8; 32]);

    let mut guardian = Guardian::deploy(
        &env,
        GuardianInitArgs {
            registry: registry.contract_address(),
        },
    );

    // NOTE: no set_guardian here. The fan-out calls pause() on the vault, which
    // reverts (the guardian holds no GUARDIAN role on it), aborting global_pause.
    env.set_caller(treasury);
    assert!(
        guardian.try_global_pause(0, 10).is_err(),
        "fan-out must fail when the vault never authorized the guardian"
    );
    assert_eq!(vault.get_status(), Status::Active, "vault stays Active");
}

#[test]
fn fanout_survives_a_vault_already_paused_out_of_band() {
    // Regression for the Wave 3 robustness gap: the registry reports a vault
    // `Active`, but its live status has diverged to `Paused` (its own agent, or an
    // earlier batch, paused it). Because the vault's `pause()` is idempotent, the
    // guardian's fan-out re-covers it as a no-op instead of reverting the entire
    // sweep — so the other vaults still get paused.
    let env = odra_test::env();
    let treasury = env.get_account(0);

    env.set_caller(treasury);
    let mut registry = VaultRegistry::deploy(&env, NoArgs);

    let mut vault_a = try_deploy(&env, happy_args(treasury)).expect("vault a deploys");
    let mut vault_b = try_deploy(&env, happy_args(treasury)).expect("vault b deploys");
    env.set_caller(treasury);
    vault_a.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault_b.with_tokens(U512::from(TOTAL_SELL)).fund();

    // Both registered Active; the guardian wired on each.
    env.set_caller(treasury);
    registry.register(vault_a.contract_address(), treasury, [1u8; 32]);
    registry.register(vault_b.contract_address(), treasury, [2u8; 32]);
    let mut guardian = Guardian::deploy(
        &env,
        GuardianInitArgs {
            registry: registry.contract_address(),
        },
    );
    env.set_caller(treasury);
    vault_a.set_guardian(guardian.contract_address());
    vault_b.set_guardian(guardian.contract_address());

    // Out-of-band: pause vault_a directly so its live status diverges from the
    // registry's Active record.
    env.set_caller(treasury);
    vault_a.pause();
    assert_eq!(vault_a.get_status(), Status::Paused);

    // The sweep must NOT revert; vault_b is still paused, vault_a stays paused.
    env.set_caller(treasury);
    let affected = guardian.global_pause(0, 10);
    assert_eq!(affected, 2, "both registry-Active records get a pause call");
    assert_eq!(
        vault_a.get_status(),
        Status::Paused,
        "already-paused vault unchanged"
    );
    assert_eq!(
        vault_b.get_status(),
        Status::Paused,
        "the other vault is paused"
    );
}
