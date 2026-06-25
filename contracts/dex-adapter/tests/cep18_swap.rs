//! Integration tests for the atomic on-chain [`Cep18SwapAdapter`].

use cadence_dex_adapter::cep18_swap::{
    Cep18SwapAdapter, Cep18SwapAdapterHostRef, Cep18SwapAdapterInitArgs, Error, PRICE_SCALE,
};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::Address;

const SELL: u64 = 100_000;
// Price 2.0 in PRICE_SCALE fixed point → 2_000_000.
const PRICE: u64 = 2 * PRICE_SCALE;
const RESERVE: u64 = 1_000_000;

struct Fixture {
    env: HostEnv,
    adapter: Cep18SwapAdapterHostRef,
    vault: Address,
    recipient: Address,
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let owner = env.get_account(0);
    let vault = env.get_account(1);
    let recipient = env.get_account(2);
    env.set_caller(owner);
    let mut adapter = Cep18SwapAdapter::deploy(
        &env,
        Cep18SwapAdapterInitArgs {
            venue_id: "cep18-pool".to_string(),
        },
    );
    adapter.set_price(U512::from(PRICE));
    adapter.with_tokens(U512::from(RESERVE)).seed_reserve();
    Fixture {
        env,
        adapter,
        vault,
        recipient,
    }
}

#[test]
fn atomic_swap_pays_recipient_and_returns_realised_amount() {
    let fx = setup();
    fx.env.set_caller(fx.vault);
    let receipt = fx.adapter.with_tokens(U512::from(SELL)).swap(
        "CSPR".to_string(),
        "USDC".to_string(),
        U512::from(SELL),
        U512::from(190_000u64),
        fx.recipient,
    );
    assert!(receipt.atomic);
    // 100_000 * 2.0 = 200_000.
    assert_eq!(receipt.bought_amount, U512::from(200_000u64));
}

#[test]
fn venue_id_is_reported() {
    let fx = setup();
    assert_eq!(fx.adapter.venue_id(), "cep18-pool".to_string());
}

#[test]
fn reverts_when_below_min_out() {
    let fx = setup();
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .with_tokens(U512::from(SELL))
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(250_000u64), // demands more than the 200_000 realised
            fx.recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::SlippageTooHigh.into());
}

#[test]
fn reverts_on_amount_mismatch() {
    let fx = setup();
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .with_tokens(U512::from(SELL - 1))
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(190_000u64),
            fx.recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::SwapAmountMismatch.into());
}

#[test]
fn reverts_when_price_unset() {
    let env = odra_test::env();
    let owner = env.get_account(0);
    let vault = env.get_account(1);
    let recipient = env.get_account(2);
    env.set_caller(owner);
    let adapter = Cep18SwapAdapter::deploy(
        &env,
        Cep18SwapAdapterInitArgs {
            venue_id: "cep18-pool".to_string(),
        },
    );
    env.set_caller(vault);
    let err = adapter
        .with_tokens(U512::from(SELL))
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(1u64),
            recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::PriceNotSet.into());
}

#[test]
fn reverts_when_reserve_insufficient() {
    let env = odra_test::env();
    let owner = env.get_account(0);
    let vault = env.get_account(1);
    let recipient = env.get_account(2);
    env.set_caller(owner);
    let mut adapter = Cep18SwapAdapter::deploy(
        &env,
        Cep18SwapAdapterInitArgs {
            venue_id: "cep18-pool".to_string(),
        },
    );
    adapter.set_price(U512::from(PRICE));
    // Reserve smaller than the 200_000 payout the swap would owe.
    adapter.with_tokens(U512::from(50_000u64)).seed_reserve();
    env.set_caller(vault);
    let err = adapter
        .with_tokens(U512::from(SELL))
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(190_000u64),
            recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::InsufficientReserve.into());
}

#[test]
fn set_price_is_owner_only() {
    let mut fx = setup();
    fx.env.set_caller(fx.vault); // not the owner
    let err = fx.adapter.try_set_price(U512::from(PRICE)).unwrap_err();
    assert_eq!(err, Error::NotOwner.into());
}

#[test]
fn set_price_rejects_zero() {
    // A zero price reads as "unset" and would brick every swap until re-set; the
    // owner must not be able to set it (foot-gun / DoS).
    let mut fx = setup();
    let owner = fx.env.get_account(0);
    fx.env.set_caller(owner);
    let err = fx.adapter.try_set_price(U512::zero()).unwrap_err();
    assert_eq!(err, Error::PriceNotSet.into());
}
