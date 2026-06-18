//! Integration tests for the `OracleAggregator` contract.
//!
//! Deploys several real `SignedPriceOracle` feeds, posts signed prices to each,
//! then verifies the aggregator's cross-call median / quorum / stale-drop logic
//! against live source contracts.

use cadence_price_oracle::aggregator::entrypoints::{
    OracleAggregatorHostRef, OracleAggregatorInitArgs,
};
use cadence_price_oracle::aggregator::{AggregatorError, OracleAggregator};
use cadence_price_oracle::signed_oracle::{
    SignedPriceOracle, SignedPriceOracleHostRef, SignedPriceOracleInitArgs,
};
use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv};
use odra::prelude::*;

const MAX_STALENESS_MS: u64 = 60_000;
const PAIR: &str = "CSPR/USDC";

/// Deploy a signed oracle whose operator is account `operator_idx`.
fn deploy_oracle(env: &HostEnv, operator_idx: usize) -> (SignedPriceOracleHostRef, Address) {
    let operator = env.get_account(operator_idx);
    env.set_caller(env.get_account(0));
    let oracle = SignedPriceOracle::deploy(
        env,
        SignedPriceOracleInitArgs {
            operator_pk: env.public_key(&operator),
            max_staleness_ms: MAX_STALENESS_MS,
        },
    );
    (oracle, operator)
}

fn preimage(oracle: &Address, pair: &str, price: U512, timestamp_ms: u64, round: u64) -> Bytes {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&oracle.to_bytes().unwrap());
    buf.extend_from_slice(&pair.to_bytes().unwrap());
    buf.extend_from_slice(&price.to_bytes().unwrap());
    buf.extend_from_slice(&timestamp_ms.to_bytes().unwrap());
    buf.extend_from_slice(&round.to_bytes().unwrap());
    Bytes::from(buf)
}

/// Post a signed price to `oracle` from `operator`, relayed by account 9.
fn post_price(
    env: &HostEnv,
    oracle: &mut SignedPriceOracleHostRef,
    operator: &Address,
    price: U512,
    timestamp_ms: u64,
    round: u64,
) {
    let msg = preimage(&oracle.address(), PAIR, price, timestamp_ms, round);
    let sig = env.sign_message(&msg, operator);
    let pk = env.public_key(operator);
    env.set_caller(env.get_account(9));
    oracle.submit_price(PAIR.to_string(), price, timestamp_ms, round, pk, sig);
}

fn deploy_aggregator(env: &HostEnv, sources: Vec<Address>, quorum: u32) -> OracleAggregatorHostRef {
    env.set_caller(env.get_account(0));
    OracleAggregator::deploy(
        env,
        OracleAggregatorInitArgs {
            sources,
            quorum,
            max_staleness_ms: MAX_STALENESS_MS,
        },
    )
}

#[test]
fn aggregates_three_sources_to_their_median() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    let (mut o2, op2) = deploy_oracle(&env, 2);
    let (mut o3, op3) = deploy_oracle(&env, 3);
    env.advance_block_time(10_000);

    post_price(&env, &mut o1, &op1, U512::from(1_000_000_000u64), 5_000, 1);
    post_price(&env, &mut o2, &op2, U512::from(1_200_000_000u64), 5_000, 1);
    post_price(&env, &mut o3, &op3, U512::from(1_500_000_000u64), 5_000, 1);

    let agg = deploy_aggregator(&env, vec![o1.address(), o2.address(), o3.address()], 2);
    let data = agg.latest_price(PAIR.to_string());
    // median(1.0, 1.2, 1.5) = 1.2
    assert_eq!(data.price, U512::from(1_200_000_000u64));
    assert_eq!(data.timestamp_ms, 5_000);
    assert_eq!(data.round, 1);
}

#[test]
fn even_count_averages_two_middle_quotes() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    let (mut o2, op2) = deploy_oracle(&env, 2);
    let (mut o3, op3) = deploy_oracle(&env, 3);
    let (mut o4, op4) = deploy_oracle(&env, 4);
    env.advance_block_time(10_000);

    post_price(&env, &mut o1, &op1, U512::from(1_000_000_000u64), 5_000, 1);
    post_price(&env, &mut o2, &op2, U512::from(2_000_000_000u64), 5_000, 1);
    post_price(&env, &mut o3, &op3, U512::from(3_000_000_000u64), 5_000, 1);
    post_price(&env, &mut o4, &op4, U512::from(4_000_000_000u64), 5_000, 1);

    let agg = deploy_aggregator(
        &env,
        vec![o1.address(), o2.address(), o3.address(), o4.address()],
        3,
    );
    // median(1,2,3,4) = (2 + 3) / 2 = 2.5
    let data = agg.latest_price(PAIR.to_string());
    assert_eq!(data.price, U512::from(2_500_000_000u64));
}

#[test]
fn drops_stale_source_but_still_meets_quorum() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    let (mut o2, op2) = deploy_oracle(&env, 2);
    let (mut o3, op3) = deploy_oracle(&env, 3);
    env.advance_block_time(10_000);

    // o1 and o2 are fresh (stamped near now); o3 is posted at an old timestamp.
    post_price(&env, &mut o1, &op1, U512::from(1_000_000_000u64), 9_000, 1);
    post_price(&env, &mut o2, &op2, U512::from(1_100_000_000u64), 9_000, 1);
    post_price(&env, &mut o3, &op3, U512::from(9_000_000_000u64), 1, 1);

    // Advance so o3 (ts=1) exceeds staleness but o1/o2 (ts=9_000) stay fresh.
    env.advance_block_time(MAX_STALENESS_MS - 1_000);

    let agg = deploy_aggregator(&env, vec![o1.address(), o2.address(), o3.address()], 2);
    let data = agg.latest_price(PAIR.to_string());
    // o3 dropped; median(1.0, 1.1) = (1.0 + 1.1)/2 = 1.05
    assert_eq!(data.price, U512::from(1_050_000_000u64));
    assert_eq!(data.timestamp_ms, 9_000);
}

#[test]
fn reverts_when_quorum_not_met_due_to_unset_sources() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    let (o2, _op2) = deploy_oracle(&env, 2);
    let (o3, _op3) = deploy_oracle(&env, 3);
    env.advance_block_time(10_000);

    // Only o1 has a price; o2 and o3 are unset.
    post_price(&env, &mut o1, &op1, U512::from(1_000_000_000u64), 5_000, 1);

    let agg = deploy_aggregator(&env, vec![o1.address(), o2.address(), o3.address()], 2);
    let err = agg.try_latest_price(PAIR.to_string()).unwrap_err();
    assert_eq!(err, AggregatorError::QuorumNotMet.into());
}

#[test]
fn reverts_when_all_sources_stale() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    let (mut o2, op2) = deploy_oracle(&env, 2);
    env.advance_block_time(10_000);
    post_price(&env, &mut o1, &op1, U512::from(1_000_000_000u64), 5_000, 1);
    post_price(&env, &mut o2, &op2, U512::from(1_100_000_000u64), 5_000, 1);

    let agg = deploy_aggregator(&env, vec![o1.address(), o2.address()], 1);
    // Age everything past the staleness bound.
    env.advance_block_time(MAX_STALENESS_MS + 10_000);
    let err = agg.try_latest_price(PAIR.to_string()).unwrap_err();
    assert_eq!(err, AggregatorError::QuorumNotMet.into());
}

#[test]
fn single_source_quorum_one_returns_that_price() {
    let env = odra_test::env();
    let (mut o1, op1) = deploy_oracle(&env, 1);
    env.advance_block_time(10_000);
    post_price(&env, &mut o1, &op1, U512::from(1_337_000_000u64), 5_000, 7);

    let agg = deploy_aggregator(&env, vec![o1.address()], 1);
    let data = agg.latest_price(PAIR.to_string());
    assert_eq!(data.price, U512::from(1_337_000_000u64));
    assert_eq!(data.round, 7);
}

#[test]
fn init_rejects_empty_sources() {
    let env = odra_test::env();
    env.set_caller(env.get_account(0));
    let result = OracleAggregator::try_deploy(
        &env,
        OracleAggregatorInitArgs {
            sources: vec![],
            quorum: 1,
            max_staleness_ms: MAX_STALENESS_MS,
        },
    );
    assert_eq!(result.err(), Some(AggregatorError::NoSources.into()));
}

#[test]
fn init_rejects_zero_quorum() {
    let env = odra_test::env();
    let (o1, _) = deploy_oracle(&env, 1);
    env.set_caller(env.get_account(0));
    let result = OracleAggregator::try_deploy(
        &env,
        OracleAggregatorInitArgs {
            sources: vec![o1.address()],
            quorum: 0,
            max_staleness_ms: MAX_STALENESS_MS,
        },
    );
    assert_eq!(result.err(), Some(AggregatorError::ZeroQuorum.into()));
}

#[test]
fn init_rejects_quorum_exceeding_sources() {
    let env = odra_test::env();
    let (o1, _) = deploy_oracle(&env, 1);
    env.set_caller(env.get_account(0));
    let result = OracleAggregator::try_deploy(
        &env,
        OracleAggregatorInitArgs {
            sources: vec![o1.address()],
            quorum: 2,
            max_staleness_ms: MAX_STALENESS_MS,
        },
    );
    assert_eq!(
        result.err(),
        Some(AggregatorError::QuorumExceedsSources.into())
    );
}

#[test]
fn init_rejects_zero_staleness() {
    let env = odra_test::env();
    let (o1, _) = deploy_oracle(&env, 1);
    env.set_caller(env.get_account(0));
    let result = OracleAggregator::try_deploy(
        &env,
        OracleAggregatorInitArgs {
            sources: vec![o1.address()],
            quorum: 1,
            max_staleness_ms: 0,
        },
    );
    assert_eq!(result.err(), Some(AggregatorError::ZeroStaleness.into()));
}

#[test]
fn exposes_configuration() {
    let env = odra_test::env();
    let (o1, _) = deploy_oracle(&env, 1);
    let (o2, _) = deploy_oracle(&env, 2);
    let agg = deploy_aggregator(&env, vec![o1.address(), o2.address()], 2);
    assert_eq!(agg.source_count(), 2);
    assert_eq!(agg.quorum(), 2);
    assert_eq!(agg.max_staleness_ms(), MAX_STALENESS_MS);
    assert_eq!(agg.source_at(0), Some(o1.address()));
    assert_eq!(agg.source_at(5), None);
}
