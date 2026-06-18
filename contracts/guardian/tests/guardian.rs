//! Integration tests for the `Guardian` desk-wide kill switch.
//!
//! The guardian fans a pause/resume out to every active vault in a real
//! [`VaultRegistry`]. We deploy the live registry, register a set of
//! [`MockVault`] contracts (each exposing the idempotent `pause`/`resume`
//! [`VaultControl`](cadence_guardian::storage::VaultControl) surface), and assert
//! the sweep flips each vault, tolerates already-paused vaults, paginates, and is
//! gated on the `GUARDIAN` role.

use cadence_guardian::errors::{Error, MAX_FANOUT_PER_CALL};
use cadence_guardian::guardian::Guardian;
use cadence_vault_registry::registry::VaultRegistry;
use cadence_vault_registry::types::VaultStatus;
use odra::host::{Deployer, HostEnv, NoArgs};
use odra::prelude::*;

/// Minimal vault stand-in exposing the guardian's `VaultControl` surface.
///
/// `pause` / `resume` are **idempotent** exactly as the trait specifies: calling
/// `pause` on an already-paused vault is a no-op that returns normally, so a
/// desk-wide sweep that re-covers it never reverts.
#[odra::module]
pub struct MockVault {
    paused: Var<bool>,
    pause_calls: Var<u32>,
}

#[odra::module]
impl MockVault {
    pub fn init(&mut self) {
        self.paused.set(false);
        self.pause_calls.set(0);
    }

    /// Idempotent pause: records the call, no-ops if already paused.
    pub fn pause(&mut self) {
        self.pause_calls.set(self.pause_calls.get_or_default() + 1);
        self.paused.set(true);
    }

    /// Idempotent resume.
    pub fn resume(&mut self) {
        self.paused.set(false);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.get_or_default()
    }

    pub fn pause_calls(&self) -> u32 {
        self.pause_calls.get_or_default()
    }
}

struct Fixture {
    env: HostEnv,
    guardian: cadence_guardian::guardian::GuardianHostRef,
    registry: cadence_vault_registry::registry::VaultRegistryHostRef,
    admin: Address,
    outsider: Address,
    vaults: Vec<MockVaultHostRef>,
}

fn mandate(tag: u8) -> [u8; 32] {
    let mut m = [0u8; 32];
    m[0] = tag;
    m
}

/// Deploy a registry, `n` mock vaults registered as `Active`, and a guardian
/// pointed at the registry. The deployer (account 0) is the bootstrap guardian.
fn setup(n: usize) -> Fixture {
    let env = odra_test::env();
    let admin = env.get_account(0);
    let treasury = env.get_account(1);
    let outsider = env.get_account(2);
    env.set_caller(admin);

    let mut registry = VaultRegistry::deploy(&env, NoArgs);
    let mut vaults: Vec<MockVaultHostRef> = Vec::new();
    for i in 0..n {
        let vault = MockVault::deploy(&env, NoArgs);
        registry.register(vault.address(), treasury, mandate(i as u8));
        vaults.push(vault);
    }

    let guardian = Guardian::deploy(
        &env,
        cadence_guardian::guardian::GuardianInitArgs {
            registry: registry.address(),
        },
    );

    Fixture {
        env,
        guardian,
        registry,
        admin,
        outsider,
        vaults,
    }
}

#[test]
fn deployer_is_the_bootstrap_guardian() {
    let fx = setup(0);
    assert!(fx.guardian.is_guardian(fx.admin));
    assert!(!fx.guardian.is_guardian(fx.outsider));
    assert!(!fx.guardian.is_paused());
    assert_eq!(fx.guardian.max_fanout_per_call(), MAX_FANOUT_PER_CALL);
}

#[test]
fn global_pause_fans_out_to_every_active_vault() {
    let mut fx = setup(3);
    let affected = fx.guardian.global_pause(0, 10);
    assert_eq!(affected, 3);
    assert!(fx.guardian.is_paused());
    for vault in &fx.vaults {
        assert!(vault.is_paused());
        assert_eq!(vault.pause_calls(), 1);
    }
}

#[test]
fn global_resume_lifts_every_paused_vault() {
    let mut fx = setup(3);
    fx.guardian.global_pause(0, 10);
    // Reflect the pause in the registry so the resume sweep sees them as Paused.
    for id in 0..3u64 {
        fx.registry.set_status(id, VaultStatus::Paused);
    }
    let affected = fx.guardian.global_resume(0, 10);
    assert_eq!(affected, 3);
    assert!(!fx.guardian.is_paused());
    for vault in &fx.vaults {
        assert!(!vault.is_paused());
    }
}

#[test]
fn tolerates_already_paused_vault() {
    let mut fx = setup(2);
    // Pre-pause the first vault directly (as its agent would), out of band.
    fx.vaults[0].pause();
    assert!(fx.vaults[0].is_paused());

    // The desk-wide sweep must still succeed and pause the rest. The registry
    // still reports both Active, so both get a (idempotent) pause call.
    let affected = fx.guardian.global_pause(0, 10);
    assert_eq!(affected, 2);
    assert!(fx.vaults[0].is_paused());
    assert!(fx.vaults[1].is_paused());
    // The already-paused vault received a second, idempotent call without reverting.
    assert_eq!(fx.vaults[0].pause_calls(), 2);
}

#[test]
fn skips_closed_vaults() {
    let mut fx = setup(3);
    // Close the middle vault in the registry; the sweep must skip it.
    fx.registry.set_status(1, VaultStatus::Closed);
    let affected = fx.guardian.global_pause(0, 10);
    assert_eq!(affected, 2);
    assert!(fx.vaults[0].is_paused());
    assert!(!fx.vaults[1].is_paused());
    assert!(fx.vaults[2].is_paused());
}

#[test]
fn pagination_sweeps_in_bounded_batches() {
    let mut fx = setup(5);
    // First page covers ids [0, 2).
    let first = fx.guardian.global_pause(0, 2);
    assert_eq!(first, 2);
    assert!(fx.guardian.is_paused());
    // Continuation pages keep sweeping without re-flipping the flag.
    let second = fx.guardian.global_pause(2, 2);
    assert_eq!(second, 2);
    let third = fx.guardian.global_pause(4, 2);
    assert_eq!(third, 1);
    for vault in &fx.vaults {
        assert!(vault.is_paused());
    }
}

#[test]
fn redundant_first_page_pause_is_rejected() {
    let mut fx = setup(1);
    fx.guardian.global_pause(0, 10);
    let err = fx.guardian.try_global_pause(0, 10).unwrap_err();
    assert_eq!(err, Error::AlreadyInState.into());
}

#[test]
fn continuation_page_before_first_page_is_rejected() {
    let mut fx = setup(3);
    // A continuation page (start > 0) while the desk is still active is out of
    // order: the flag is not yet in the paused state.
    let err = fx.guardian.try_global_pause(1, 2).unwrap_err();
    assert_eq!(err, Error::AlreadyInState.into());
}

#[test]
fn zero_limit_is_rejected() {
    let mut fx = setup(2);
    let err = fx.guardian.try_global_pause(0, 0).unwrap_err();
    assert_eq!(err, Error::InvalidBatchBound.into());
}

#[test]
fn over_cap_limit_is_rejected() {
    let mut fx = setup(2);
    let err = fx
        .guardian
        .try_global_pause(0, MAX_FANOUT_PER_CALL + 1)
        .unwrap_err();
    assert_eq!(err, Error::InvalidBatchBound.into());
}

#[test]
fn non_guardian_cannot_pause() {
    let mut fx = setup(2);
    fx.env.set_caller(fx.outsider);
    let err = fx.guardian.try_global_pause(0, 10).unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn rotate_guardian_moves_authority() {
    let mut fx = setup(0);
    fx.env.set_caller(fx.admin);
    fx.guardian.rotate_guardian(fx.outsider);
    assert!(fx.guardian.is_guardian(fx.outsider));
    assert!(!fx.guardian.is_guardian(fx.admin));

    // The new guardian can pause; the old one can no longer.
    fx.env.set_caller(fx.admin);
    let err = fx.guardian.try_global_pause(0, 1).unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn plain_guardian_can_rotate_again() {
    // Two consecutive hand-offs: deployer -> A, then A -> B where A holds only
    // GUARDIAN (not ROOT_ADMIN). Proves rotation does not require role-admin.
    let mut fx = setup(0);
    let a = fx.outsider;
    let b = fx.env.get_account(3);

    fx.env.set_caller(fx.admin);
    fx.guardian.rotate_guardian(a);
    assert!(fx.guardian.is_guardian(a));

    // A is a plain GUARDIAN with no ROOT_ADMIN, yet can hand off to B.
    fx.env.set_caller(a);
    fx.guardian.rotate_guardian(b);
    assert!(fx.guardian.is_guardian(b));
    assert!(!fx.guardian.is_guardian(a));
}

#[test]
fn rotate_to_self_is_rejected() {
    let mut fx = setup(0);
    fx.env.set_caller(fx.admin);
    let err = fx.guardian.try_rotate_guardian(fx.admin).unwrap_err();
    assert_eq!(err, Error::AlreadyInState.into());
}

#[test]
fn empty_registry_pause_flips_flag_with_no_fanout() {
    let mut fx = setup(0);
    let affected = fx.guardian.global_pause(0, 10);
    assert_eq!(affected, 0);
    assert!(fx.guardian.is_paused());
}
