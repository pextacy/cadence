//! Pure unit tests for `cadence-common`.
//!
//! These pin the extracted math to the vault's current on-chain behaviour. The
//! slippage / price scenarios reuse the exact numbers from
//! `contracts/vault/src/vault.rs` tests so any divergence shows up here.

use odra::casper_types::U512;

use cadence_common::checked::{checked_add, checked_div, checked_mul, checked_sub, MathError};
use cadence_common::fee::{fee_amount, net_after_fee};
use cadence_common::price::{check_price_band, implied_price, within_band, PriceError};
use cadence_common::scale::{
    apply_price, rescale, BPS_DENOMINATOR, PRICE_SCALE, PRICE_SCALE_1E6, PRICE_SCALE_1E9,
};
use cadence_common::slippage::{check_slippage, within_slippage, SlippageError};

fn u(v: u64) -> U512 {
    U512::from(v)
}

// ---------------------------------------------------------------------------
// checked arithmetic
// ---------------------------------------------------------------------------

#[test]
fn checked_add_sums() {
    assert_eq!(checked_add(u(2), u(3)), Ok(u(5)));
}

#[test]
fn checked_add_detects_overflow() {
    assert_eq!(checked_add(U512::MAX, u(1)), Err(MathError::Overflow));
}

#[test]
fn checked_sub_subtracts() {
    assert_eq!(checked_sub(u(5), u(3)), Ok(u(2)));
}

#[test]
fn checked_sub_detects_underflow() {
    assert_eq!(checked_sub(u(3), u(5)), Err(MathError::Underflow));
}

#[test]
fn checked_mul_multiplies() {
    assert_eq!(checked_mul(u(4), u(5)), Ok(u(20)));
}

#[test]
fn checked_mul_detects_overflow() {
    assert_eq!(checked_mul(U512::MAX, u(2)), Err(MathError::Overflow));
}

#[test]
fn checked_div_divides_truncating() {
    assert_eq!(checked_div(u(7), u(2)), Ok(u(3))); // truncates toward zero
}

#[test]
fn checked_div_detects_zero_divisor() {
    assert_eq!(checked_div(u(7), u(0)), Err(MathError::DivByZero));
}

// ---------------------------------------------------------------------------
// scale constants + conversions
// ---------------------------------------------------------------------------

#[test]
fn scale_constants_match_existing_contracts() {
    assert_eq!(PRICE_SCALE_1E9, 1_000_000_000);
    assert_eq!(PRICE_SCALE_1E6, 1_000_000);
    assert_eq!(PRICE_SCALE, PRICE_SCALE_1E9); // alias = vault scale
    assert_eq!(BPS_DENOMINATOR, 10_000);
}

#[test]
fn apply_price_matches_dex_adapter_quote() {
    // cep18_swap: bought = sell * price / 1e6. Price 2.0 (=2_000_000) on 50 sell.
    let bought = apply_price(u(50), u(2 * PRICE_SCALE_1E6), PRICE_SCALE_1E6).unwrap();
    assert_eq!(bought, u(100));
}

#[test]
fn apply_price_at_vault_scale() {
    // Price 2.0 at 1e9 on sell 100_000 → 200_000.
    let out = apply_price(u(100_000), u(2 * PRICE_SCALE_1E9), PRICE_SCALE_1E9).unwrap();
    assert_eq!(out, u(200_000));
}

#[test]
fn rescale_bridges_1e6_and_1e9() {
    // 1.0 expressed at 1e6 → at 1e9.
    assert_eq!(rescale(u(PRICE_SCALE_1E6), PRICE_SCALE_1E6, PRICE_SCALE_1E9), Ok(u(PRICE_SCALE_1E9)));
    // and back.
    assert_eq!(rescale(u(PRICE_SCALE_1E9), PRICE_SCALE_1E9, PRICE_SCALE_1E6), Ok(u(PRICE_SCALE_1E6)));
}

// ---------------------------------------------------------------------------
// slippage — numbers lifted from vault tests
// ---------------------------------------------------------------------------

#[test]
fn slippage_accepts_exactly_one_percent() {
    // vault ok_slice: quote 200_000, min_out 198_000, cap 100 bps (1%). Passes.
    assert_eq!(check_slippage(u(200_000), u(198_000), 100), Ok(()));
    assert_eq!(within_slippage(u(200_000), u(198_000), 100), Ok(true));
}

#[test]
fn slippage_rejects_one_point_five_percent() {
    // vault rejects_excessive_slippage: min_out 197_000 of 200_000 => 1.5% > 1%.
    assert_eq!(
        check_slippage(u(200_000), u(197_000), 100),
        Err(SlippageError::SlippageTooHigh)
    );
    assert_eq!(within_slippage(u(200_000), u(197_000), 100), Ok(false));
}

#[test]
fn slippage_rejects_min_out_above_quote() {
    // vault Error::MinOutAboveQuote.
    assert_eq!(
        check_slippage(u(200_000), u(200_001), 100),
        Err(SlippageError::MinOutAboveQuote)
    );
}

#[test]
fn slippage_zero_min_out_within_cap_when_full_allowed() {
    // 100% slippage cap (10_000 bps) accepts any min_out down to zero.
    assert_eq!(check_slippage(u(200_000), u(0), 10_000), Ok(()));
}

#[test]
fn slippage_zero_cap_requires_exact_quote() {
    // 0 bps cap: only min_out == quote passes.
    assert_eq!(check_slippage(u(200_000), u(200_000), 0), Ok(()));
    assert_eq!(
        check_slippage(u(200_000), u(199_999), 0),
        Err(SlippageError::SlippageTooHigh)
    );
}

// ---------------------------------------------------------------------------
// price band — numbers lifted from vault tests
// ---------------------------------------------------------------------------

#[test]
fn implied_price_matches_vault_two_point_zero() {
    // quote 200_000 over sell 100_000 at 1e9 → price 2.0 == 2_000_000_000.
    assert_eq!(
        implied_price(u(200_000), u(100_000)),
        Ok(u(2_000_000_000))
    );
}

#[test]
fn implied_price_zero_sell_amount_is_div_by_zero() {
    assert_eq!(
        implied_price(u(200_000), u(0)),
        Err(MathError::DivByZero)
    );
}

#[test]
fn within_band_treats_zero_bounds_as_unset() {
    // Both bounds unset → always in band (vault happy path uses 0/0).
    assert!(within_band(u(3_000_000_000), u(0), u(0)));
    // Only floor set.
    assert!(within_band(u(2_000_000_000), u(1_500_000_000), u(0)));
    assert!(!within_band(u(1_000_000_000), u(1_500_000_000), u(0)));
    // Only ceiling set.
    assert!(within_band(u(2_000_000_000), u(0), u(2_500_000_000)));
    assert!(!within_band(u(3_000_000_000), u(0), u(2_500_000_000)));
}

#[test]
fn price_band_rejects_price_above_ceiling() {
    // vault rejects_price_outside_band: band [1.5, 2.5], quote priced 3.0.
    // sell 100_000, quote 300_000 → price 3.0 == 3_000_000_000 > ceiling 2.5.
    assert_eq!(
        check_price_band(
            u(300_000),
            u(100_000),
            u(1_500_000_000),
            u(2_500_000_000),
        ),
        Err(PriceError::OutOfBand)
    );
}

#[test]
fn price_band_accepts_in_band() {
    // price 2.0 inside [1.5, 2.5].
    assert_eq!(
        check_price_band(
            u(200_000),
            u(100_000),
            u(1_500_000_000),
            u(2_500_000_000),
        ),
        Ok(())
    );
}

#[test]
fn price_band_boundaries_are_inclusive() {
    // price exactly == floor and == ceiling both pass (>=, <=).
    assert_eq!(
        check_price_band(u(150_000), u(100_000), u(1_500_000_000), u(2_500_000_000)),
        Ok(())
    );
    assert_eq!(
        check_price_band(u(250_000), u(100_000), u(1_500_000_000), u(2_500_000_000)),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// fee
// ---------------------------------------------------------------------------

#[test]
fn fee_amount_basic_bps() {
    // 0.25% of 1_000_000 = 2_500.
    assert_eq!(fee_amount(u(1_000_000), 25), Ok(u(2_500)));
    // 1% of 1_000_000 = 10_000.
    assert_eq!(fee_amount(u(1_000_000), 100), Ok(u(10_000)));
}

#[test]
fn fee_amount_zero_bps_is_zero() {
    assert_eq!(fee_amount(u(1_000_000), 0), Ok(u(0)));
}

#[test]
fn fee_amount_rounds_down() {
    // 1 bps of 5 = 0.0005 → truncates to 0.
    assert_eq!(fee_amount(u(5), 1), Ok(u(0)));
    // 33 bps of 1000 = 3.3 → 3.
    assert_eq!(fee_amount(u(1000), 33), Ok(u(3)));
}

#[test]
fn fee_full_bps_takes_whole_amount() {
    // 100% (10_000 bps) → entire amount is fee, net zero.
    assert_eq!(fee_amount(u(1_000_000), 10_000), Ok(u(1_000_000)));
    assert_eq!(net_after_fee(u(1_000_000), 10_000), Ok(u(0)));
}

#[test]
fn net_after_fee_deducts() {
    assert_eq!(net_after_fee(u(1_000_000), 100), Ok(u(990_000)));
}

#[test]
fn fee_amount_overflow_is_caught() {
    assert_eq!(fee_amount(U512::MAX, 10_000), Err(MathError::Overflow));
}
