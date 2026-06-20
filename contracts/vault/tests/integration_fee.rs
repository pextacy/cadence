//! Wave 2b item A integration: the vault accrues a protocol fee on each slice
//! fill — but ONLY when a fee module is wired via `set_fee_module`.
//!
//! Proves the full optional path: treasury deploys a real `FeeModule`, grants the
//! vault the fee-collector role, and wires it via `set_fee_module`. The agent then
//! executes a slice that settles atomically through the `Cep18SwapAdapter`; the
//! vault records the fill and, last (checks-effects-interactions), calls
//! `accrue_fee` cross-contract on the realised buy amount. The fee module's accrued
//! balance for the vault must equal `bought_amount * fee_bps / 10_000`.
//!
//! A control test runs the identical flow with NO fee module set and asserts the
//! fill still works and nothing is accrued — proving fee accrual is truly optional.

mod common;

use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::Address;

use cadence_dex_adapter::cep18_swap::{
    Cep18SwapAdapter, Cep18SwapAdapterHostRef, Cep18SwapAdapterInitArgs,
};
use cadence_fee_module::fees::FeeModuleInitArgs;
use cadence_fee_module::{FeeModule, FeeModuleHostRef};
use cadence_vault::vault::{ExecutionVault, ExecutionVaultHostRef, ExecutionVaultInitArgs};

use common::*;

const VENUE: &str = "cspr.trade";
// 1e6-scale price of 2.0 → bought = sell * 2. (The cep18 adapter uses PRICE_SCALE_1E6.)
const ADAPTER_PRICE: u64 = 2_000_000;
const FEE_BPS: u32 = 25; // 0.25%

/// Deploy the atomic adapter + a vault whose single venue address is that adapter,
/// fund the vault, and opt the venue into adapter routing. Mirrors the scaffolding
/// in `integration_adapter.rs`. The fee module is wired separately by each test so
/// the no-fee control can reuse this unchanged.
fn setup() -> (
    HostEnv,
    ExecutionVaultHostRef,
    Cep18SwapAdapterHostRef,
    Address,
) {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    env.set_caller(treasury);

    // 1. Deploy the atomic adapter, set its price, seed its payout reserve.
    let mut adapter = Cep18SwapAdapter::deploy(
        &env,
        Cep18SwapAdapterInitArgs {
            venue_id: VENUE.to_string(),
        },
    );
    adapter.set_price(U512::from(ADAPTER_PRICE));
    adapter.with_tokens(U512::from(TOTAL_SELL)).seed_reserve();

    // 2. Sign a mandate whose venue address IS the adapter, and deploy the vault.
    let adapter_addr = adapter.contract_address();
    let preimage = mandate_message_offchain(
        agent,
        treasury,
        "CSPR",
        "USDC",
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        &venues(),
        &[adapter_addr],
        &nonce32(),
    );
    let casper_signature = env.sign_message(&preimage, &treasury);
    let mut vault = ExecutionVault::deploy(
        &env,
        ExecutionVaultInitArgs {
            agent,
            mandate_digest: digest32(),
            signature: odra::casper_types::bytesrepr::Bytes::from(vec![1u8; 65]),
            treasury_public_key: env.public_key(&treasury),
            casper_signature,
            mandate_nonce: nonce32(),
            sell_asset: "CSPR".to_string(),
            buy_asset: "USDC".to_string(),
            total_sell: U512::from(TOTAL_SELL),
            end_time_ms: END_TIME_MS,
            max_slippage_bps: SLIPPAGE_BPS,
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            venues: venues(),
            venue_addresses: vec![adapter_addr],
        },
    );

    // 3. Fund the vault, then opt the venue into cross-contract adapter routing.
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault.set_venue_adapter(VENUE.to_string(), true);

    (env, vault, adapter, adapter_addr)
}

/// Deploy a real `FeeModule` (treasury is the deployer/admin), grant the vault the
/// fee-collector role, and return it. The granting entrypoint is `grant_collector`.
fn deploy_fee_module_for(
    env: &HostEnv,
    treasury: Address,
    vault_addr: Address,
) -> FeeModuleHostRef {
    env.set_caller(treasury);
    let mut fee_module = FeeModule::deploy(
        env,
        FeeModuleInitArgs {
            init_fee_bps: FEE_BPS,
        },
    );
    // The VAULT is the cross-contract caller of `accrue_fee`, so the vault — not the
    // treasury — must hold the collector role on the module.
    fee_module.grant_collector(vault_addr);
    assert!(fee_module.is_collector(vault_addr));
    fee_module
}

#[test]
fn fill_accrues_protocol_fee_when_fee_module_is_wired() {
    let (env, mut vault, _adapter, _adapter_addr) = setup();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    let vault_addr = vault.contract_address();

    // Wire a real fee module and grant the vault the collector role on it.
    let fee_module = deploy_fee_module_for(&env, treasury, vault_addr);
    env.set_caller(treasury);
    vault.set_fee_module(fee_module.contract_address());

    // Agent executes a slice that settles atomically: bought = 200_000.
    env.set_caller(agent);
    vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );

    let bought = U512::from(200_000u64);
    assert_eq!(vault.get_bought_so_far(), bought);

    // The vault accrued `bought * fee_bps / 10_000` to ITS OWN ledger entry.
    let expected_fee = bought * U512::from(FEE_BPS) / U512::from(10_000u64);
    assert_eq!(expected_fee, U512::from(500u64), "0.25% of 200_000 = 500");
    assert_eq!(
        fee_module.accrued_of(vault_addr),
        expected_fee,
        "fee module must credit bought_amount * fee_bps / 10_000 to the vault"
    );
}

#[test]
fn fill_accrues_nothing_when_no_fee_module_is_set() {
    // Control: identical flow but `set_fee_module` is NEVER called. The fill must
    // still succeed and nothing is accrued — proving fee accrual is truly optional.
    let (env, mut vault, _adapter, _adapter_addr) = setup();
    let agent = env.get_account(1);
    let vault_addr = vault.contract_address();

    // Deploy a fee module purely as an observer; it is NOT wired into the vault.
    let treasury = env.get_account(0);
    let fee_module = deploy_fee_module_for(&env, treasury, vault_addr);

    env.set_caller(agent);
    vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );

    // The fill still works...
    assert_eq!(vault.get_bought_so_far(), U512::from(200_000u64));
    // ...and nothing was accrued anywhere, because no fee module was wired.
    assert_eq!(
        fee_module.accrued_of(vault_addr),
        U512::zero(),
        "no fee module wired ⇒ no accrual, fill must be unaffected"
    );
}
