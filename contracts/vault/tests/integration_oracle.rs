//! Wave 5 integration: the vault's optional oracle cross-check. With a
//! `SignedPriceOracle` wired, a slice whose implied price tracks the oracle passes,
//! while a slice that deviates beyond the configured tolerance is reverted on-chain
//! — defence in depth on top of the static mandate band.

mod common;

use common::{try_deploy, DeployArgs, TOTAL_SELL};

use cadence_price_oracle::signed_oracle::{
    SignedPriceOracle, SignedPriceOracleHostRef, SignedPriceOracleInitArgs,
};
use cadence_vault::vault::Error as VaultError;
use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::{Address, Addressable};

const PAIR: &str = "CSPR-USDC";
// 2.0 at the 1e9 price scale — matches a 200_000-out / 100_000-in slice.
const TWO: u64 = 2_000_000_000;
const THREE: u64 = 3_000_000_000;

fn price_message(oracle: &Address, pair: &str, price: U512, ts: u64, round: u64) -> Bytes {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&oracle.to_bytes().unwrap());
    buf.extend_from_slice(&pair.to_bytes().unwrap());
    buf.extend_from_slice(&price.to_bytes().unwrap());
    buf.extend_from_slice(&ts.to_bytes().unwrap());
    buf.extend_from_slice(&round.to_bytes().unwrap());
    Bytes::from(buf)
}

/// Deploy an oracle and seed one signed price for `PAIR`.
fn deploy_oracle_with_price(env: &HostEnv, price: u64) -> SignedPriceOracleHostRef {
    let operator = env.get_account(3);
    let relayer = env.get_account(4);
    env.set_caller(env.get_account(0));
    let mut oracle: SignedPriceOracleHostRef = SignedPriceOracle::deploy(
        env,
        SignedPriceOracleInitArgs {
            operator_pk: env.public_key(&operator),
            max_staleness_ms: 60_000,
        },
    );
    env.advance_block_time(10_000);
    let ts = 5_000u64;
    let p = U512::from(price);
    let msg = price_message(&oracle.address(), PAIR, p, ts, 1);
    let sig = env.sign_message(&msg, &operator);
    let pk = env.public_key(&operator);
    env.set_caller(relayer);
    oracle.submit_price(PAIR.to_string(), p, ts, 1, pk, sig);
    oracle
}

/// Deploy + fund a Direct-venue vault in the given env (treasury=acct0, agent=acct1).
fn deploy_funded_vault(env: &HostEnv) -> cadence_vault::vault::ExecutionVaultHostRef {
    let treasury = env.get_account(0);
    let vault = try_deploy(
        env,
        DeployArgs {
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            signer: treasury,
            supplied_pk_account: treasury,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: None,
        },
    )
    .expect("happy-path deploy must verify");
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault
}

#[test]
fn slice_within_oracle_tolerance_passes() {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);

    let oracle = deploy_oracle_with_price(&env, TWO);
    let mut vault = deploy_funded_vault(&env);

    env.set_caller(treasury);
    vault.set_oracle(oracle.address(), PAIR.to_string(), 100); // 1% tolerance

    // Implied price 2.0 == oracle 2.0 → within band.
    env.set_caller(agent);
    let slice_id = vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        "cspr.trade".to_string(),
    );
    assert_eq!(slice_id, 0);
    assert_eq!(vault.get_sold_so_far(), U512::from(100_000u64));
}

#[test]
fn slice_outside_oracle_tolerance_reverts() {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);

    // Oracle says 3.0; the slice implies 2.0 — a 33% deviation, well over tolerance.
    let oracle = deploy_oracle_with_price(&env, THREE);
    let mut vault = deploy_funded_vault(&env);

    env.set_caller(treasury);
    vault.set_oracle(oracle.address(), PAIR.to_string(), 100);

    env.set_caller(agent);
    let err = vault
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, VaultError::OracleBandBreach.into());
}

#[test]
fn set_oracle_is_treasury_only() {
    let env = odra_test::env();
    let agent = env.get_account(1);
    let oracle = deploy_oracle_with_price(&env, TWO);
    let mut vault = deploy_funded_vault(&env);

    env.set_caller(agent);
    assert!(vault
        .try_set_oracle(oracle.address(), PAIR.to_string(), 100)
        .is_err());
}
