//! Integration tests for the Cadence RBAC + pausable primitives, exercised
//! through the deployable [`AccessControlContract`] wrapper.

use cadence_access_control::contract::{AccessControlContract, AccessControlContractHostRef};
use cadence_access_control::{roles, Error};
use odra::host::{Deployer, HostEnv, NoArgs};
use odra::prelude::Address;

struct Fixture {
    env: HostEnv,
    contract: AccessControlContractHostRef,
    admin: Address,
    alice: Address,
    bob: Address,
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let admin = env.get_account(0);
    let alice = env.get_account(1);
    let bob = env.get_account(2);
    env.set_caller(admin);
    let contract = AccessControlContract::deploy(&env, NoArgs);
    Fixture { env, contract, admin, alice, bob }
}

#[test]
fn deployer_is_root_admin_and_has_defaults() {
    let fx = setup();
    assert!(fx.contract.has_role(roles::ROOT_ADMIN, fx.admin));
    assert!(fx.contract.has_role(roles::TREASURY, fx.admin));
    assert!(fx.contract.has_role(roles::GUARDIAN, fx.admin));
    assert!(!fx.contract.has_role(roles::AGENT, fx.admin));
}

#[test]
fn root_admin_can_grant_any_role() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::AGENT, fx.alice);
    assert!(fx.contract.has_role(roles::AGENT, fx.alice));
}

#[test]
fn non_admin_cannot_grant_role() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice); // alice holds nothing
    let err = fx.contract.try_grant_role(roles::AGENT, fx.bob).unwrap_err();
    assert_eq!(err, Error::NotRoleAdmin.into());
    assert!(!fx.contract.has_role(roles::AGENT, fx.bob));
}

#[test]
fn revoke_removes_the_role() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::AGENT, fx.alice);
    assert!(fx.contract.has_role(roles::AGENT, fx.alice));
    fx.contract.revoke_role(roles::AGENT, fx.alice);
    assert!(!fx.contract.has_role(roles::AGENT, fx.alice));
}

#[test]
fn non_admin_cannot_revoke() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::AGENT, fx.alice);
    fx.env.set_caller(fx.bob);
    let err = fx.contract.try_revoke_role(roles::AGENT, fx.alice).unwrap_err();
    assert_eq!(err, Error::NotRoleAdmin.into());
    assert!(fx.contract.has_role(roles::AGENT, fx.alice));
}

#[test]
fn assert_role_reverts_for_non_holder() {
    let fx = setup();
    let err = fx.contract.try_assert_role(roles::TREASURY, fx.alice).unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn assert_role_passes_for_holder() {
    let fx = setup();
    // admin holds TREASURY by default — assert must not revert.
    fx.contract.assert_role(roles::TREASURY, fx.admin);
}

#[test]
fn two_step_transfer_grants_to_recipient_on_accept() {
    let mut fx = setup();
    // Give alice the AGENT role first so she can transfer it.
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::AGENT, fx.alice);

    fx.env.set_caller(fx.alice);
    fx.contract.begin_transfer_role(roles::AGENT, fx.bob);
    assert_eq!(fx.contract.pending_role(roles::AGENT), Some(fx.bob));
    // Not granted until accepted.
    assert!(!fx.contract.has_role(roles::AGENT, fx.bob));

    fx.env.set_caller(fx.bob);
    fx.contract.accept_role(roles::AGENT);
    assert!(fx.contract.has_role(roles::AGENT, fx.bob));
    assert_eq!(fx.contract.pending_role(roles::AGENT), None);
}

#[test]
fn only_pending_recipient_may_accept() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::AGENT, fx.alice);
    fx.env.set_caller(fx.alice);
    fx.contract.begin_transfer_role(roles::AGENT, fx.bob);
    // admin (not the pending recipient) tries to accept.
    fx.env.set_caller(fx.admin);
    let err = fx.contract.try_accept_role(roles::AGENT).unwrap_err();
    assert_eq!(err, Error::NotPendingRecipient.into());
}

#[test]
fn accept_without_pending_reverts() {
    let mut fx = setup();
    fx.env.set_caller(fx.bob);
    let err = fx.contract.try_accept_role(roles::AGENT).unwrap_err();
    assert_eq!(err, Error::NoPendingTransfer.into());
}

#[test]
fn begin_transfer_requires_holding_the_role() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice); // alice does not hold AGENT
    let err = fx
        .contract
        .try_begin_transfer_role(roles::AGENT, fx.bob)
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn set_role_admin_delegates_administration() {
    let mut fx = setup();
    // Make TREASURY the admin of AGENT. admin holds TREASURY + ROOT_ADMIN,
    // and ROOT_ADMIN is the current admin of AGENT, so admin may set it.
    fx.env.set_caller(fx.admin);
    fx.contract.set_role_admin(roles::AGENT, roles::TREASURY);
    assert_eq!(fx.contract.get_role_admin(roles::AGENT), roles::TREASURY);

    // Now grant TREASURY to alice; alice (a treasury) may grant AGENT.
    fx.contract.grant_role(roles::TREASURY, fx.alice);
    fx.env.set_caller(fx.alice);
    fx.contract.grant_role(roles::AGENT, fx.bob);
    assert!(fx.contract.has_role(roles::AGENT, fx.bob));
}

#[test]
fn guardian_can_pause_and_unpause() {
    let mut fx = setup();
    assert!(!fx.contract.is_paused());
    fx.env.set_caller(fx.admin); // admin holds GUARDIAN by default
    fx.contract.set_paused(true);
    assert!(fx.contract.is_paused());
    fx.contract.set_paused(false);
    assert!(!fx.contract.is_paused());
}

#[test]
fn non_guardian_cannot_pause() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice); // alice is not a guardian
    let err = fx.contract.try_set_paused(true).unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
    assert!(!fx.contract.is_paused());
}

#[test]
fn granted_guardian_can_pause() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_role(roles::GUARDIAN, fx.alice);
    fx.env.set_caller(fx.alice);
    fx.contract.set_paused(true);
    assert!(fx.contract.is_paused());
}
