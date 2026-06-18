//! Integration tests for the `SignedPriceOracle` contract.

use cadence_price_oracle::errors::Error;
use cadence_price_oracle::signed_oracle::{
    SignedPriceOracle, SignedPriceOracleHostRef, SignedPriceOracleInitArgs,
};
use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::{PublicKey, U512};
use odra::host::{Deployer, HostEnv};
use odra::prelude::*;

const MAX_STALENESS_MS: u64 = 60_000;
const PAIR: &str = "CSPR/USDC";

struct Fixture {
    env: HostEnv,
    oracle: SignedPriceOracleHostRef,
    operator: Address,
    relayer: Address,
    stranger: Address,
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let deployer = env.get_account(0);
    let operator = env.get_account(1);
    let relayer = env.get_account(2);
    let stranger = env.get_account(3);
    env.set_caller(deployer);
    let oracle = SignedPriceOracle::deploy(
        &env,
        SignedPriceOracleInitArgs {
            operator_pk: env.public_key(&operator),
            max_staleness_ms: MAX_STALENESS_MS,
        },
    );
    Fixture {
        env,
        oracle,
        operator,
        relayer,
        stranger,
    }
}

/// Reconstruct the exact preimage the contract signs over, off-chain.
fn price_message(
    oracle: &Address,
    pair: &str,
    price: U512,
    timestamp_ms: u64,
    round: u64,
) -> Bytes {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&oracle.to_bytes().unwrap());
    buf.extend_from_slice(&pair.to_bytes().unwrap());
    buf.extend_from_slice(&price.to_bytes().unwrap());
    buf.extend_from_slice(&timestamp_ms.to_bytes().unwrap());
    buf.extend_from_slice(&round.to_bytes().unwrap());
    Bytes::from(buf)
}

/// Build a valid (publicKey, signature) for a price from the operator.
fn sign_price(
    fx: &Fixture,
    signer: &Address,
    pair: &str,
    price: U512,
    timestamp_ms: u64,
    round: u64,
) -> (PublicKey, Bytes) {
    let oracle_addr = fx.oracle.address();
    let msg = price_message(&oracle_addr, pair, price, timestamp_ms, round);
    let sig = fx.env.sign_message(&msg, signer);
    let pk = fx.env.public_key(signer);
    (pk, sig)
}

#[test]
fn accepts_a_signed_price_and_reads_it_back() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_250_000_000u64); // 1.25 at PRICE_SCALE
    let ts = 5_000u64;
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, ts, 1);

    // A relayer (not the operator) submits the signed update.
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, ts, 1, pk, sig);

    let data = fx.oracle.latest_price(PAIR.to_string());
    assert_eq!(data.price, price);
    assert_eq!(data.timestamp_ms, ts);
    assert_eq!(data.round, 1);
    assert_eq!(fx.oracle.last_round(PAIR.to_string()), 1);
}

#[test]
fn rejects_zero_price() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, U512::zero(), 5_000, 1);
    fx.env.set_caller(fx.relayer);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), U512::zero(), 5_000, 1, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::ZeroPrice.into());
}

#[test]
fn rejects_unauthorized_signer() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_000_000_000u64);
    // Signed by a stranger, not the registered operator.
    let (pk, sig) = sign_price(&fx, &fx.stranger, PAIR, price, 5_000, 1);
    fx.env.set_caller(fx.relayer);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::NotAuthorizedSigner.into());
}

#[test]
fn rejects_tampered_price() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let signed_price = U512::from(1_000_000_000u64);
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, signed_price, 5_000, 1);
    // Submit a different price than was signed → signature no longer matches.
    fx.env.set_caller(fx.relayer);
    let err = fx
        .oracle
        .try_submit_price(
            PAIR.to_string(),
            U512::from(2_000_000_000u64),
            5_000,
            1,
            pk,
            sig,
        )
        .unwrap_err();
    assert_eq!(err, Error::BadSignature.into());
}

#[test]
fn rejects_out_of_order_round() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_000_000_000u64);
    let (pk1, sig1) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 5);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 5_000, 5, pk1, sig1);

    // Round 4 < 5 → rejected even with a fresher timestamp.
    let (pk2, sig2) = sign_price(&fx, &fx.operator, PAIR, price, 6_000, 4);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 6_000, 4, pk2, sig2)
        .unwrap_err();
    assert_eq!(err, Error::StaleRound.into());
}

#[test]
fn rejects_equal_round_replay() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_000_000_000u64);
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 5_000, 1, pk.clone(), sig.clone());
    // Same round replayed.
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::StaleRound.into());
}

#[test]
fn rejects_future_timestamp() {
    let mut fx = setup();
    fx.env.advance_block_time(5_000);
    let price = U512::from(1_000_000_000u64);
    // timestamp 9_000 > block time 5_000.
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 9_000, 1);
    fx.env.set_caller(fx.relayer);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 9_000, 1, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::TimestampInFuture.into());
}

#[test]
fn rejects_non_increasing_timestamp() {
    let mut fx = setup();
    fx.env.advance_block_time(20_000);
    let price = U512::from(1_000_000_000u64);
    let (pk1, sig1) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 1);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 10_000, 1, pk1, sig1);
    // Higher round but a stale (equal) timestamp → rejected.
    let (pk2, sig2) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 2);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 10_000, 2, pk2, sig2)
        .unwrap_err();
    assert_eq!(err, Error::StaleTimestamp.into());
}

#[test]
fn latest_price_reverts_when_unset() {
    let fx = setup();
    let err = fx.oracle.try_latest_price(PAIR.to_string()).unwrap_err();
    assert_eq!(err, Error::NoPrice.into());
}

#[test]
fn latest_price_reverts_when_stale() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_000_000_000u64);
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig);
    // Age now 5_000ms; advance well past the 60_000ms bound.
    fx.env.advance_block_time(MAX_STALENESS_MS + 10_000);
    let err = fx.oracle.try_latest_price(PAIR.to_string()).unwrap_err();
    assert_eq!(err, Error::StalePrice.into());
}

#[test]
fn fresh_within_bound_is_readable() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let price = U512::from(1_000_000_000u64);
    // Stamp the price at the current block time, then advance to just under
    // the staleness edge so the read is still fresh.
    let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 1);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 10_000, 1, pk, sig);
    fx.env.advance_block_time(MAX_STALENESS_MS - 1);
    let data = fx.oracle.latest_price(PAIR.to_string());
    assert_eq!(data.price, price);
}

#[test]
fn rotate_operator_changes_authorized_signer() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let new_op = fx.env.get_account(4);

    // Only the current operator may rotate.
    fx.env.set_caller(fx.stranger);
    let err = fx
        .oracle
        .try_rotate_operator(fx.env.public_key(&new_op))
        .unwrap_err();
    assert_eq!(err, Error::NotOperator.into());

    fx.env.set_caller(fx.operator);
    fx.oracle.rotate_operator(fx.env.public_key(&new_op));
    assert_eq!(fx.oracle.operator(), new_op);

    // Old operator's signature is now rejected.
    let price = U512::from(1_000_000_000u64);
    let (old_pk, old_sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
    fx.env.set_caller(fx.relayer);
    let err = fx
        .oracle
        .try_submit_price(PAIR.to_string(), price, 5_000, 1, old_pk, old_sig)
        .unwrap_err();
    assert_eq!(err, Error::NotAuthorizedSigner.into());

    // New operator's signature is accepted.
    let (new_pk, new_sig) = sign_price(&fx, &new_op, PAIR, price, 5_000, 1);
    fx.oracle
        .submit_price(PAIR.to_string(), price, 5_000, 1, new_pk, new_sig);
    assert_eq!(fx.oracle.latest_price(PAIR.to_string()).round, 1);
}

#[test]
fn rejects_double_init() {
    let mut fx = setup();
    // Odra blocks calling a constructor entrypoint a second time at the VM
    // level (InvalidContext), which is the actual double-init protection;
    // the in-contract AlreadyInitialised guard is defence-in-depth.
    let result = fx
        .oracle
        .try_init(fx.env.public_key(&fx.operator), MAX_STALENESS_MS);
    assert!(result.is_err());
}

#[test]
fn rejects_zero_staleness_init() {
    let env = odra_test::env();
    let deployer = env.get_account(0);
    let operator = env.get_account(1);
    env.set_caller(deployer);
    let result = SignedPriceOracle::try_deploy(
        &env,
        SignedPriceOracleInitArgs {
            operator_pk: env.public_key(&operator),
            max_staleness_ms: 0,
        },
    );
    assert_eq!(result.err(), Some(Error::ZeroStaleness.into()));
}

#[test]
fn supports_multiple_independent_pairs() {
    let mut fx = setup();
    fx.env.advance_block_time(10_000);
    let p1 = U512::from(1_000_000_000u64);
    let p2 = U512::from(2_500_000_000u64);
    let (pk1, sig1) = sign_price(&fx, &fx.operator, "CSPR/USDC", p1, 5_000, 1);
    let (pk2, sig2) = sign_price(&fx, &fx.operator, "BTC/USDC", p2, 5_000, 1);
    fx.env.set_caller(fx.relayer);
    fx.oracle
        .submit_price("CSPR/USDC".to_string(), p1, 5_000, 1, pk1, sig1);
    fx.oracle
        .submit_price("BTC/USDC".to_string(), p2, 5_000, 1, pk2, sig2);
    assert_eq!(fx.oracle.latest_price("CSPR/USDC".to_string()).price, p1);
    assert_eq!(fx.oracle.latest_price("BTC/USDC".to_string()).price, p2);
}
