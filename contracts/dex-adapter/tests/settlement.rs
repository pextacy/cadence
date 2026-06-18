//! Integration tests for the escrow + attested-settlement [`SettlementAdapter`],
//! plus a golden-vector test pinning the FROZEN settlement preimage byte layout.

use cadence_dex_adapter::adapter::SwapReceipt;
use cadence_dex_adapter::settlement::preimage::settlement_message;
use cadence_dex_adapter::settlement::{
    Error, SettlementAdapter, SettlementAdapterHostRef, SettlementAdapterInitArgs,
};
use odra::casper_types::account::AccountHash;
use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::{PublicKey, U512};
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::{Address, Addressable};

const SELL: u64 = 100_000;
const MIN_OUT: u64 = 198_000;

// ---------------------------------------------------------------------------
// Golden vector for the FROZEN settlement preimage.
//
// `settlement_message` is consensus-critical: a live operator's signing key was
// issued against this exact byte layout. This test pins the bytes for a fully
// deterministic input so any reorder / re-encode / field change is caught.
// ---------------------------------------------------------------------------

/// Deterministic address built from a fixed 32-byte account hash — no env needed.
fn fixed_address(byte: u8) -> Address {
    Address::from(AccountHash::new([byte; 32]))
}

#[test]
fn settlement_message_golden_vector() {
    let adapter = fixed_address(0xAA);
    let recipient = fixed_address(0xBB);
    let escrow_id: u64 = 7;
    let bought_amount = U512::from(199_000u64);
    let settlement_ref = Bytes::from(b"deploy-hash-abc".to_vec());
    let nonce = Bytes::from(vec![9u8; 32]);

    let msg = settlement_message(
        &adapter,
        escrow_id,
        bought_amount,
        &settlement_ref,
        &nonce,
        &recipient,
    )
    .expect("preimage serializes");

    // Independent reconstruction of the canonical layout: adapter || escrow_id ||
    // bought_amount || settlement_ref || nonce || recipient, each in Casper ToBytes.
    let mut expected: Vec<u8> = Vec::new();
    expected.extend_from_slice(&adapter.to_bytes().unwrap());
    expected.extend_from_slice(&escrow_id.to_bytes().unwrap());
    expected.extend_from_slice(&bought_amount.to_bytes().unwrap());
    expected.extend_from_slice(&settlement_ref.to_bytes().unwrap());
    expected.extend_from_slice(&nonce.to_bytes().unwrap());
    expected.extend_from_slice(&recipient.to_bytes().unwrap());

    assert_eq!(msg.as_slice(), expected.as_slice());

    // Pinned golden hex — frozen. If this assertion fails the preimage layout drifted.
    let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, GOLDEN_PREIMAGE_HEX);
}

/// FROZEN golden vector for `settlement_message` with the inputs above. Layout:
/// `adapter(0xAA*32, account variant tag 0x00) || escrow_id=7 (u64 LE) ||
/// bought_amount=199_000 (U512 ToBytes) || settlement_ref "deploy-hash-abc"
/// (len-prefixed) || nonce 0x09*32 (len-prefixed) || recipient(0xBB*32)`.
const GOLDEN_PREIMAGE_HEX: &str = "00aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0700000000000000035809030f0000006465706c6f792d686173682d61626320000000090909090909090909090909090909090909090909090909090909090909090900bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// ---------------------------------------------------------------------------
// Behavioural tests against a deployed adapter.
// ---------------------------------------------------------------------------

struct Fixture {
    env: HostEnv,
    adapter: SettlementAdapterHostRef,
    vault: Address,
    operator: Address,
    recipient: Address,
}

fn setup() -> Fixture {
    let env = odra_test::env();
    let operator = env.get_account(0);
    let vault = env.get_account(1);
    let recipient = env.get_account(2);
    env.set_caller(operator);
    let adapter = SettlementAdapter::deploy(
        &env,
        SettlementAdapterInitArgs {
            venue_id: "cspr.trade".to_string(),
            operator,
        },
    );
    Fixture {
        env,
        adapter,
        vault,
        operator,
        recipient,
    }
}

/// The vault (caller) escrows a slice, attaching the native sell amount.
fn escrow(fx: &mut Fixture) -> SwapReceipt {
    fx.env.set_caller(fx.vault);
    fx.adapter.with_tokens(U512::from(SELL)).swap(
        "CSPR".to_string(),
        "USDC".to_string(),
        U512::from(SELL),
        U512::from(MIN_OUT),
        fx.recipient,
    )
}

fn sign_settlement(
    fx: &Fixture,
    escrow_id: u64,
    bought_amount: U512,
    settlement_ref: &Bytes,
    nonce: &Bytes,
) -> (PublicKey, Bytes) {
    let addr = fx.adapter.address();
    // Reconstruct the exact preimage the contract signs over via the shared FROZEN
    // builder, so the test cannot drift from production.
    let msg = settlement_message(
        &addr,
        escrow_id,
        bought_amount,
        settlement_ref,
        nonce,
        &fx.recipient,
    )
    .expect("preimage serializes");
    let sig = fx.env.sign_message(&msg, &fx.operator);
    let pk = fx.env.public_key(&fx.operator);
    (pk, sig)
}

#[test]
fn swap_escrows_and_returns_non_atomic_receipt() {
    let mut fx = setup();
    let receipt = escrow(&mut fx);
    assert!(!receipt.atomic);
    assert_eq!(receipt.bought_amount, U512::zero());

    let escrow = fx.adapter.get_escrow(0).expect("escrow booked");
    assert_eq!(escrow.sell_amount, U512::from(SELL));
    assert_eq!(escrow.min_out, U512::from(MIN_OUT));
    assert_eq!(escrow.recipient, fx.recipient);
    assert!(!escrow.settled);
}

#[test]
fn venue_id_is_reported() {
    let fx = setup();
    assert_eq!(fx.adapter.venue_id(), "cspr.trade".to_string());
}

#[test]
fn rejects_escrow_amount_mismatch() {
    let fx = setup();
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .with_tokens(U512::from(SELL - 1)) // attach less than declared
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(MIN_OUT),
            fx.recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::EscrowAmountMismatch.into());
}

#[test]
fn rejects_zero_sell_amount() {
    let mut fx = setup();
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .try_swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::zero(),
            U512::from(MIN_OUT),
            fx.recipient,
        )
        .unwrap_err();
    assert_eq!(err, Error::ZeroSellAmount.into());
}

#[test]
fn operator_attestation_settles_the_escrow() {
    let mut fx = setup();
    escrow(&mut fx);

    let nonce = Bytes::from(vec![9u8; 32]);
    let settlement_ref = Bytes::from(b"deploy-hash-abc".to_vec());
    let bought = U512::from(199_000u64);
    let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);

    // Anyone may submit it; only a valid operator signature authorises it.
    fx.env.set_caller(fx.vault);
    fx.adapter
        .record_settlement(0, bought, settlement_ref, nonce.clone(), pk, sig);

    let escrow = fx.adapter.get_escrow(0).unwrap();
    assert!(escrow.settled);
    assert!(fx.adapter.attestation_used(fx.operator, nonce));
}

#[test]
fn rejects_settlement_below_min_out() {
    let mut fx = setup();
    escrow(&mut fx);
    let nonce = Bytes::from(vec![1u8; 32]);
    let settlement_ref = Bytes::from(b"ref".to_vec());
    let bought = U512::from(MIN_OUT - 1); // below committed floor
    let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::SlippageTooHigh.into());
}

#[test]
fn rejects_tampered_bought_amount() {
    let mut fx = setup();
    escrow(&mut fx);
    let nonce = Bytes::from(vec![2u8; 32]);
    let settlement_ref = Bytes::from(b"ref".to_vec());
    let signed = U512::from(199_000u64);
    let (pk, sig) = sign_settlement(&fx, 0, signed, &settlement_ref, &nonce);
    // Submit a different bought amount than was signed → signature mismatch.
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .try_record_settlement(0, U512::from(250_000u64), settlement_ref, nonce, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::BadSignature.into());
}

#[test]
fn rejects_wrong_signer() {
    let mut fx = setup();
    escrow(&mut fx);
    let nonce = Bytes::from(vec![3u8; 32]);
    let settlement_ref = Bytes::from(b"ref".to_vec());
    let bought = U512::from(199_000u64);
    let addr = fx.adapter.address();
    let msg =
        settlement_message(&addr, 0, bought, &settlement_ref, &nonce, &fx.recipient).unwrap();
    // Sign with a non-operator account.
    let sig = fx.env.sign_message(&msg, &fx.recipient);
    let pk = fx.env.public_key(&fx.recipient);
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::NotAuthorizedSigner.into());
}

#[test]
fn rejects_replayed_attestation() {
    let mut fx = setup();
    escrow(&mut fx);
    // Escrow a second slice so the first settlement does not exhaust everything.
    escrow(&mut fx);

    let nonce = Bytes::from(vec![4u8; 32]);
    let settlement_ref = Bytes::from(b"ref".to_vec());
    let bought = U512::from(199_000u64);
    let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);
    fx.env.set_caller(fx.vault);
    fx.adapter.record_settlement(
        0,
        bought,
        settlement_ref.clone(),
        nonce.clone(),
        pk.clone(),
        sig.clone(),
    );
    // Re-submitting the same escrow is blocked by the settled flag.
    let err = fx
        .adapter
        .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::EscrowAlreadySettled.into());
}

#[test]
fn rejects_reused_nonce_across_escrows() {
    let mut fx = setup();
    escrow(&mut fx);
    escrow(&mut fx);

    let nonce = Bytes::from(vec![5u8; 32]);
    let ref0 = Bytes::from(b"ref0".to_vec());
    let bought = U512::from(199_000u64);
    let (pk0, sig0) = sign_settlement(&fx, 0, bought, &ref0, &nonce);
    fx.env.set_caller(fx.vault);
    fx.adapter
        .record_settlement(0, bought, ref0, nonce.clone(), pk0, sig0);

    // Settle escrow 1 reusing the same nonce → replay-protected.
    let ref1 = Bytes::from(b"ref1".to_vec());
    let (pk1, sig1) = sign_settlement(&fx, 1, bought, &ref1, &nonce);
    let err = fx
        .adapter
        .try_record_settlement(1, bought, ref1, nonce, pk1, sig1)
        .unwrap_err();
    assert_eq!(err, Error::AttestationAlreadyUsed.into());
}

#[test]
fn rejects_unknown_escrow() {
    let mut fx = setup();
    let nonce = Bytes::from(vec![6u8; 32]);
    let settlement_ref = Bytes::from(b"ref".to_vec());
    let bought = U512::from(199_000u64);
    let (pk, sig) = sign_settlement(&fx, 7, bought, &settlement_ref, &nonce);
    fx.env.set_caller(fx.vault);
    let err = fx
        .adapter
        .try_record_settlement(7, bought, settlement_ref, nonce, pk, sig)
        .unwrap_err();
    assert_eq!(err, Error::UnknownEscrow.into());
}
