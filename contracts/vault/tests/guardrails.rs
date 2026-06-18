//! Guardrail coverage: the on-chain rejection paths exercised through
//! `execute_slice` / `record_fill`, plus direct unit tests of the pure predicates
//! in `cadence_vault::vault::guardrails` (which delegate to `cadence-common`).

mod common;

use odra::casper_types::U512;

use cadence_vault::vault::guardrails;
use cadence_vault::vault::Error;
use common::*;

// ---------------------------------------------------------------------------
// On-chain guardrail rejection paths
// ---------------------------------------------------------------------------

#[test]
fn rejects_over_spend_cap() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.agent);
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(TOTAL_SELL + 1),
            U512::from(2_000_002u64),
            U512::from(1_980_001u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::SpendCapExceeded.into());
}

#[test]
fn rejects_past_deadline() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.advance_block_time(END_TIME_MS + 1);
    fx.env.set_caller(fx.agent);
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::DeadlinePassed.into());
}

#[test]
fn rejects_excessive_slippage() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.agent);
    // min_out 197_000 of quote 200_000 => 1.5% > 1% cap.
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(197_000u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::SlippageTooHigh.into());
}

#[test]
fn rejects_non_allowlisted_venue() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.agent);
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "evil.dex".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::VenueNotAllowed.into());
}

#[test]
fn rejects_price_outside_band() {
    // Band [1.5, 2.5]; a quote priced at 3.0 must revert.
    let mut fx = deploy_with(U512::from(1_500_000_000u64), U512::from(2_500_000_000u64));
    fund(&mut fx);
    fx.env.set_caller(fx.agent);
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(300_000u64), // price 3.0
            U512::from(300_000u64), // zero slippage so only price check fails
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::PriceOutOfBand.into());
}

#[test]
fn rejects_double_fill() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    ok_slice(&mut fx);
    fx.env.set_caller(fx.agent);
    fx.contract
        .record_fill(0, U512::from(199_000u64), "deploy-hash-1".to_string());
    let err = fx
        .contract
        .try_record_fill(0, U512::from(199_000u64), "deploy-hash-2".to_string())
        .unwrap_err();
    assert_eq!(err, Error::SliceAlreadyFilled.into());
}

// ---------------------------------------------------------------------------
// Pure predicate unit tests (environment-free)
// ---------------------------------------------------------------------------

#[test]
fn spend_cap_predicate_matches_contract() {
    let total = U512::from(TOTAL_SELL);
    // sold + amount within cap.
    let new_sold = guardrails::add_sold(U512::from(900_000u64), U512::from(100_000u64)).unwrap();
    assert!(guardrails::check_spend_cap(new_sold, total).is_ok());
    // one wei over the cap.
    let over = guardrails::add_sold(U512::from(900_000u64), U512::from(100_001u64)).unwrap();
    assert_eq!(
        guardrails::check_spend_cap(over, total),
        Err(Error::SpendCapExceeded)
    );
}

#[test]
fn slippage_predicate_matches_contract() {
    // Exactly 1% (the cap) passes.
    assert!(guardrails::check_slice_slippage(
        U512::from(200_000u64),
        U512::from(198_000u64),
        SLIPPAGE_BPS,
    )
    .is_ok());
    // 1.5% fails.
    assert_eq!(
        guardrails::check_slice_slippage(
            U512::from(200_000u64),
            U512::from(197_000u64),
            SLIPPAGE_BPS,
        ),
        Err(Error::SlippageTooHigh)
    );
    // min_out above quote is nonsensical.
    assert_eq!(
        guardrails::check_slice_slippage(
            U512::from(200_000u64),
            U512::from(200_001u64),
            SLIPPAGE_BPS,
        ),
        Err(Error::MinOutAboveQuote)
    );
}

#[test]
fn price_band_predicate_matches_contract() {
    let floor = U512::from(1_500_000_000u64);
    let ceiling = U512::from(2_500_000_000u64);
    // price 2.0 inside band.
    assert!(guardrails::check_slice_price(
        U512::from(200_000u64),
        U512::from(100_000u64),
        floor,
        ceiling,
    )
    .is_ok());
    // price 3.0 above ceiling.
    assert_eq!(
        guardrails::check_slice_price(
            U512::from(300_000u64),
            U512::from(100_000u64),
            floor,
            ceiling,
        ),
        Err(Error::PriceOutOfBand)
    );
    // zero bounds == unset, anything passes.
    assert!(guardrails::check_slice_price(
        U512::from(300_000u64),
        U512::from(100_000u64),
        U512::zero(),
        U512::zero(),
    )
    .is_ok());
}

#[test]
fn fill_min_out_predicate_matches_contract() {
    assert!(guardrails::check_fill_min_out(U512::from(199_000u64), U512::from(198_000u64)).is_ok());
    assert_eq!(
        guardrails::check_fill_min_out(U512::from(197_000u64), U512::from(198_000u64)),
        Err(Error::SlippageTooHigh)
    );
}
