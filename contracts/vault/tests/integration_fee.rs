//! Wave 2b item A integration: the vault accrues a protocol fee on slice fills —
//! but accrual is DECOUPLED from fill recording so a fee-module fault can never
//! block a legitimate fill (CLAUDE.md §4.5/§4.6).
//!
//! A recorded fill only accumulates its realised buy amount into the vault's
//! `pending_fee_base` locally; the cross-contract `accrue_fee` call happens solely
//! in the separate, retriable `flush_fees` entrypoint. Two tests cover the design:
//! the happy accumulate→flush path, and the finding-#1 regression proving a
//! reverting/unauthorised fee module cannot block `record_fill`.

mod common;

use odra::casper_types::U512;
use odra::host::{Deployer, HostRef};

use cadence_fee_module::fees::FeeModuleInitArgs;
use cadence_fee_module::{FeeModule, FeeModuleHostRef};

use common::*;

const FEE_BPS: u32 = 25; // 0.25%
const BOUGHT: u64 = 200_000;

/// Expected fee for `BOUGHT` at `FEE_BPS`: bought * bps / 10_000.
fn expected_fee() -> U512 {
    U512::from(BOUGHT) * U512::from(FEE_BPS) / U512::from(10_000u64)
}

/// Deploy a real `FeeModule` (treasury is admin) and grant the vault the collector
/// role so its cross-contract `accrue_fee` is authorised.
fn deploy_fee_module(fx: &Fixture, vault_addr: odra::prelude::Address) -> FeeModuleHostRef {
    fx.env.set_caller(fx.treasury);
    let mut fee_module = FeeModule::deploy(
        &fx.env,
        FeeModuleInitArgs {
            init_fee_bps: FEE_BPS,
        },
    );
    fee_module.grant_collector(vault_addr);
    fee_module
}

#[test]
fn flush_accrues_pending_fee_after_fill() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    let vault_addr = fx.contract.contract_address();

    let fee_module = deploy_fee_module(&fx, vault_addr);
    fx.env.set_caller(fx.treasury);
    fx.contract.set_fee_module(fee_module.contract_address());

    // A slice + fill only ACCUMULATES the obligation locally — nothing is accrued
    // on the module yet (the fee call is decoupled into flush_fees).
    let id = ok_slice(&mut fx);
    fx.env.set_caller(fx.agent);
    fx.contract
        .record_fill(id, U512::from(BOUGHT), "deploy-hash".to_string());
    assert_eq!(fx.contract.get_pending_fee_base(), U512::from(BOUGHT));
    assert_eq!(fee_module.accrued_of(vault_addr), U512::zero());

    // Flushing pushes the accumulated base to the module and resets the pending base.
    fx.env.set_caller(fx.agent);
    fx.contract.flush_fees();
    assert_eq!(fee_module.accrued_of(vault_addr), expected_fee());
    assert_eq!(fx.contract.get_pending_fee_base(), U512::zero());
}

#[test]
fn record_fill_is_never_blocked_by_a_reverting_fee_module() {
    // Finding-#1 regression. On the off-chain path execute_slice releases the sell
    // asset to the venue in one tx; the agent proves the fill later via record_fill.
    // Even with the vault's collector role revoked (so the module reverts on
    // accrue_fee), record_fill MUST succeed — fills never call the module.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    let vault_addr = fx.contract.contract_address();

    let mut fee_module = deploy_fee_module(&fx, vault_addr);
    fx.env.set_caller(fx.treasury);
    fx.contract.set_fee_module(fee_module.contract_address());
    // Revoke the role so any cross-contract accrue_fee would revert Unauthorized.
    fx.env.set_caller(fx.treasury);
    fee_module.revoke_collector(vault_addr);
    assert!(!fee_module.is_collector(vault_addr));

    // Slice releases funds, then the fill is recorded — must not revert.
    let id = ok_slice(&mut fx);
    fx.env.set_caller(fx.agent);
    fx.contract
        .record_fill(id, U512::from(BOUGHT), "deploy-hash".to_string());
    assert_eq!(fx.contract.get_bought_so_far(), U512::from(BOUGHT));
    assert_eq!(fx.contract.get_pending_fee_base(), U512::from(BOUGHT));

    // The external fee call is isolated to flush_fees, which DOES revert now...
    fx.env.set_caller(fx.agent);
    assert!(
        fx.contract.try_flush_fees().is_err(),
        "flush must surface the module fault"
    );
    // ...yet the obligation survived. Restore the role and the retry settles it.
    fx.env.set_caller(fx.treasury);
    fee_module.grant_collector(vault_addr);
    fx.env.set_caller(fx.agent);
    fx.contract.flush_fees();
    assert_eq!(fee_module.accrued_of(vault_addr), expected_fee());
    assert_eq!(fx.contract.get_pending_fee_base(), U512::zero());
}
