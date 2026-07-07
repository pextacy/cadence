//! Wave 2b integration: the vault settles a slice cross-contract through a
//! `VenueAdapter` (the atomic `Cep18SwapAdapter`) instead of a blind transfer.
//!
//! Proves the full path: treasury opts a venue into adapter routing, the agent
//! executes a slice, the vault attaches the sell amount and calls the adapter via
//! the generated `VenueAdapterContractRef`, the adapter pays the realised buy
//! amount to the treasury, and the vault records the fill atomically in the same
//! call (no separate `record_fill`).

mod common;

use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::Address;

use cadence_dex_adapter::cep18_swap::{
    Cep18SwapAdapter, Cep18SwapAdapterHostRef, Cep18SwapAdapterInitArgs,
};
use cadence_fee_module::{FeeModule, FeeModuleInitArgs};
use cadence_vault::vault::{ExecutionVault, ExecutionVaultHostRef, ExecutionVaultInitArgs};

use common::*;

const VENUE: &str = "cspr.trade";
// 1e6-scale price of 2.0 → bought = sell * 2. (The cep18 adapter uses PRICE_SCALE_1E6.)
const ADAPTER_PRICE: u64 = 2_000_000;

/// Deploy a Cep18SwapAdapter (priced + reserve-seeded) and a vault whose single
/// venue address is that adapter, then fund the vault and opt the venue into
/// adapter routing. Returns the live refs.
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
    let vault = ExecutionVault::deploy(
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
    let mut vault = vault;
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault.set_venue_adapter(VENUE.to_string(), true);

    (env, vault, adapter, treasury)
}

#[test]
fn slice_settles_atomically_through_the_adapter() {
    let (env, mut vault, _adapter, treasury) = setup();
    let agent = env.get_account(1);

    let treasury_before = env.balance_of(&treasury);

    // Agent executes a slice: sell 100_000, quote 200_000, min_out 198_000 (1%).
    env.set_caller(agent);
    let slice_id = vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );
    assert_eq!(slice_id, 0);

    // The adapter priced 2.0 → bought 200_000, recorded ATOMICALLY in the same
    // call (no separate record_fill needed).
    assert_eq!(
        vault.get_bought_so_far(),
        U512::from(200_000u64),
        "atomic fill must be recorded inside execute_slice"
    );
    assert_eq!(vault.get_sold_so_far(), U512::from(100_000u64));
    assert_eq!(vault.get_slice_count(), 1);

    // The realised buy asset was paid to the treasury (non-custodial), not the agent.
    let treasury_after = env.balance_of(&treasury);
    assert_eq!(
        treasury_after - treasury_before,
        U512::from(200_000u64),
        "adapter must pay the realised buy amount to the treasury"
    );
}

#[test]
fn record_fill_is_rejected_after_atomic_settlement() {
    // An atomically-settled slice is already filled, so a stray record_fill for it
    // must revert rather than double-count.
    let (env, mut vault, _adapter, _treasury) = setup();
    let agent = env.get_account(1);

    env.set_caller(agent);
    let slice_id = vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );

    let err = vault
        .try_record_fill(slice_id, U512::from(200_000u64), "dup".to_string())
        .unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::SliceAlreadyFilled.into());
}

#[test]
fn fill_accrues_the_protocol_fee_to_the_configured_collector() {
    // Opt-in protocol fee: with a fee module wired, an atomic fill accrues the
    // module's bps fee on the realised buy amount to a distinct collector account
    // (not the trading treasury), proving the end-to-end cross-contract accrual.
    let (env, mut vault, _adapter, treasury) = setup();
    let agent = env.get_account(1);
    let collector = env.get_account(2);
    const FEE_BPS: u32 = 50; // 0.5%

    // Treasury deploys the fee module (it becomes ROOT_ADMIN + FEE_COLLECTOR), then
    // grants the vault the collector role so the vault may accrue on its behalf.
    env.set_caller(treasury);
    let mut fee = FeeModule::deploy(
        &env,
        FeeModuleInitArgs {
            init_fee_bps: FEE_BPS,
        },
    );
    env.set_caller(treasury);
    fee.grant_collector(vault.contract_address());

    // Treasury opts the vault into fees, crediting the configured collector.
    env.set_caller(treasury);
    vault.set_fee_module(fee.contract_address(), collector);
    assert_eq!(vault.get_fee_module(), Some(fee.contract_address()));

    // Agent fills a slice atomically: bought = 200_000 → fee = 200_000 * 50/10_000 = 1000.
    env.set_caller(agent);
    vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );
    assert_eq!(vault.get_bought_so_far(), U512::from(200_000u64));
    assert_eq!(
        fee.accrued_of(collector),
        U512::from(1_000u64),
        "the protocol fee must accrue to the configured collector on the fill"
    );
}

#[test]
fn fills_do_not_accrue_when_no_fee_module_is_configured() {
    // Fees are off by default: a fill with no fee module wired must not touch any
    // fee ledger (the default path is unchanged for existing mandates).
    let (env, mut vault, _adapter, _treasury) = setup();
    let agent = env.get_account(1);
    env.set_caller(agent);
    vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    );
    // No fee module configured → the optional accrual was skipped (no revert, fill ok).
    assert_eq!(vault.get_fee_module(), None);
    assert_eq!(vault.get_bought_so_far(), U512::from(200_000u64));
}
