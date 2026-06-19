//! Wave 5 integration: the vault's optional oracle price-deviation cross-check.
//! When the treasury configures an oracle, `execute_slice` additionally requires
//! the slice's implied price to be within a deviation band of the oracle's price
//! — proven here by calling the vault cross-contract against a mock `OracleAdapter`
//! returning a controlled price (the real SignedPriceOracle/Aggregator expose the
//! same `latest_price(pair)` interface and are tested in the price-oracle crate).

mod common;

use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::*;

use cadence_price_oracle::types::PriceData;
use cadence_vault::vault::{Error, ExecutionVaultHostRef, Status};

use common::*;

/// Minimal `OracleAdapter` stand-in returning a fixed price. The entrypoint name
/// and `pair` parameter MUST match the `OracleAdapter` trait so the vault's
/// `OracleAdapterContractRef` dispatches to it (Odra routes args by name).
#[odra::module]
pub struct MockOracle {
    price: Var<U512>,
}

#[odra::module]
impl MockOracle {
    pub fn init(&mut self, price: U512) {
        self.price.set(price);
    }

    pub fn latest_price(&self, pair: String) -> PriceData {
        let _ = pair;
        PriceData {
            price: self.price.get_or_default(),
            timestamp_ms: 0,
            round: 1,
        }
    }
}

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

/// Deploy a mock oracle priced at `oracle_price` (1e9 scale) + a funded vault that
/// cross-checks against it within `max_dev_bps`.
fn setup(oracle_price: u64, max_dev_bps: u32) -> (HostEnv, ExecutionVaultHostRef) {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    env.set_caller(treasury);

    let oracle = MockOracle::deploy(
        &env,
        MockOracleInitArgs {
            price: U512::from(oracle_price),
        },
    );
    let mut vault = try_deploy(&env, happy_args(treasury)).expect("vault deploys");
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault.set_oracle(
        oracle.contract_address(),
        "CSPR/USDC".to_string(),
        max_dev_bps,
    );

    (env, vault)
}

#[test]
fn slice_passes_when_within_oracle_band() {
    // A 100_000-for-200_000 quote implies a price of 2.0 (= 2e9 at 1e9 scale),
    // exactly the oracle's price, so the 1% band is satisfied.
    let (env, mut vault) = setup(2_000_000_000, 100);
    let agent = env.get_account(1);
    env.set_caller(agent);
    let id = vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        "cspr.trade".to_string(),
    );
    assert_eq!(id, 0);
    assert_eq!(vault.get_sold_so_far(), U512::from(100_000u64));
    assert_eq!(vault.get_status(), Status::Active);
}

#[test]
fn slice_reverts_when_outside_oracle_band() {
    // Oracle says 2.5 while the slice implies 2.0 — a 25% deviation, far beyond
    // the 1% band, so the on-chain cross-check rejects the slice.
    let (env, mut vault) = setup(2_500_000_000, 100);
    let agent = env.get_account(1);
    env.set_caller(agent);
    let err = vault
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::OraclePriceDeviation.into());
    assert_eq!(
        vault.get_sold_so_far(),
        U512::zero(),
        "rejected slice must not advance progress"
    );
}
