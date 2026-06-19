//! Integration tests for the Cadence [`FeeModule`], exercised through the
//! deployable contract host ref.

use cadence_access_control::Error as AcError;
use cadence_fee_module::fees::{FeeModuleInitArgs, MAX_FEE_BPS};
use cadence_fee_module::{Error, FeeModule, FeeModuleHostRef};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv};
use odra::prelude::Address;

const INIT_BPS: u32 = 25; // 0.25%

struct Fixture {
    env: HostEnv,
    contract: FeeModuleHostRef,
    admin: Address,
    alice: Address,
    bob: Address,
}

fn setup() -> Fixture {
    setup_with_bps(INIT_BPS)
}

fn setup_with_bps(bps: u32) -> Fixture {
    let env = odra_test::env();
    let admin = env.get_account(0);
    let alice = env.get_account(1);
    let bob = env.get_account(2);
    env.set_caller(admin);
    let contract = FeeModule::deploy(&env, FeeModuleInitArgs { init_fee_bps: bps });
    Fixture {
        env,
        contract,
        admin,
        alice,
        bob,
    }
}

fn u(n: u64) -> U512 {
    U512::from(n)
}

#[test]
fn deployer_is_collector_and_rate_is_set() {
    let fx = setup();
    assert_eq!(fx.contract.fee_bps(), INIT_BPS);
    assert!(fx.contract.is_collector(fx.admin));
    assert!(!fx.contract.is_collector(fx.alice));
}

#[test]
fn deploy_above_max_rate_reverts() {
    let env = odra_test::env();
    let admin = env.get_account(0);
    env.set_caller(admin);
    let result = FeeModule::try_deploy(
        &env,
        FeeModuleInitArgs {
            init_fee_bps: MAX_FEE_BPS + 1,
        },
    );
    assert!(result.is_err());
}

#[test]
fn accrue_credits_collector_and_returns_fee() {
    let mut fx = setup();
    // 0.25% of 1_000_000 = 2_500.
    let fee = fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    assert_eq!(fee, u(2_500));
    assert_eq!(fx.contract.accrued_of(fx.admin), u(2_500));
}

#[test]
fn accrue_is_cumulative() {
    let mut fx = setup();
    fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    assert_eq!(fx.contract.accrued_of(fx.admin), u(5_000));
}

#[test]
fn fee_rounds_down() {
    // 0.25% of 399 = 0.9975 → truncated to 0.
    let mut fx = setup();
    let fee = fx.contract.accrue_fee("USDC".to_string(), u(399));
    assert_eq!(fee, U512::zero());
}

#[test]
fn accrue_zero_amount_reverts() {
    let mut fx = setup();
    let err = fx
        .contract
        .try_accrue_fee("USDC".to_string(), U512::zero())
        .unwrap_err();
    assert_eq!(err, Error::ZeroAmount.into());
}

#[test]
fn non_collector_cannot_accrue() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice);
    let err = fx
        .contract
        .try_accrue_fee("USDC".to_string(), u(1_000_000))
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn set_fee_bps_updates_rate() {
    let mut fx = setup();
    fx.contract.set_fee_bps(50);
    assert_eq!(fx.contract.fee_bps(), 50);
    let fee = fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    assert_eq!(fee, u(5_000));
}

#[test]
fn set_fee_bps_above_max_reverts() {
    let mut fx = setup();
    let err = fx.contract.try_set_fee_bps(MAX_FEE_BPS + 1).unwrap_err();
    assert_eq!(err, Error::FeeRateTooHigh.into());
    assert_eq!(fx.contract.fee_bps(), INIT_BPS);
}

#[test]
fn non_collector_cannot_set_fee_bps() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice);
    let err = fx.contract.try_set_fee_bps(50).unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn withdraw_zeroes_balance_and_returns_amount() {
    let mut fx = setup();
    fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    let amount = fx.contract.withdraw(fx.bob);
    assert_eq!(amount, u(2_500));
    assert_eq!(fx.contract.accrued_of(fx.admin), U512::zero());
}

#[test]
fn withdraw_with_nothing_accrued_reverts() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice);
    let err = fx.contract.try_withdraw(fx.alice).unwrap_err();
    assert_eq!(err, Error::NothingAccrued.into());
}

#[test]
fn granted_collector_can_accrue_to_own_ledger() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_collector(fx.alice);
    assert!(fx.contract.is_collector(fx.alice));

    fx.env.set_caller(fx.alice);
    let fee = fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    assert_eq!(fee, u(2_500));
    assert_eq!(fx.contract.accrued_of(fx.alice), u(2_500));
    // admin's ledger is untouched.
    assert_eq!(fx.contract.accrued_of(fx.admin), U512::zero());
}

#[test]
fn revoked_collector_cannot_accrue() {
    let mut fx = setup();
    fx.env.set_caller(fx.admin);
    fx.contract.grant_collector(fx.alice);
    fx.contract.revoke_collector(fx.alice);

    fx.env.set_caller(fx.alice);
    let err = fx
        .contract
        .try_accrue_fee("USDC".to_string(), u(1_000_000))
        .unwrap_err();
    assert_eq!(err, Error::Unauthorized.into());
}

#[test]
fn non_admin_cannot_grant_collector() {
    let mut fx = setup();
    fx.env.set_caller(fx.alice);
    let err = fx.contract.try_grant_collector(fx.bob).unwrap_err();
    // AC raises NotRoleAdmin (code 2) — distinct from the module's Unauthorized.
    assert_eq!(err, AcError::NotRoleAdmin.into());
    assert!(!fx.contract.is_collector(fx.bob));
}

#[test]
fn zero_rate_accrues_nothing() {
    let mut fx = setup_with_bps(0);
    let fee = fx.contract.accrue_fee("USDC".to_string(), u(1_000_000));
    assert_eq!(fee, U512::zero());
    assert_eq!(fx.contract.accrued_of(fx.admin), U512::zero());
}
