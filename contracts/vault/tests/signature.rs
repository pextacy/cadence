//! On-chain mandate-signature verification and the FROZEN preimage golden vector.
//!
//! The golden-vector test pins the exact bytes of the canonical mandate preimage
//! for a fixed input. Because the preimage is signed off-chain and verified
//! on-chain (and re-derived in TypeScript), ANY change to its field order or
//! `ToBytes` framing is consensus-breaking — this test fails loudly if the bytes
//! drift, guarding the decomposition's "preimage is frozen" invariant.

mod common;

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;

use common::*;

// ---------------------------------------------------------------------------
// FROZEN preimage golden vector
// ---------------------------------------------------------------------------

/// Hex of the canonical preimage for the fixed input below. Captured from the
/// pre-decomposition `mandate_message`. If this assertion fails, the preimage
/// byte layout changed — that is a breaking change to every signed mandate and
/// must be intentional.
const GOLDEN_PREIMAGE_HEX: &str = "436164656e63652d4d616e646174652d763100105b69f2d74a211a6cb337cba6751a8f15cc7b44b7c65329c29731b67e1ac047000ee624f1bbaec23e6dc6b877815f322058045c45cdbf8ef53e7db2c58b0af309040000004353505204000000555344430340420f40420f0000000000640000000000010000000a000000637370722e74726164650100000000147f2cc33b4fdb04ab4e9ef2c067137177097ba50a544a0a343ce636028fcfcf200000000505050505050505050505050505050505050505050505050505050505050505";

#[test]
fn preimage_golden_vector_is_frozen() {
    let env = odra_test::env();
    let agent = env.get_account(1);
    let treasury = env.get_account(0);
    let venue_addr = env.get_account(2);

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
        &[venue_addr],
        &nonce32(),
    );
    let hex = hex_encode(preimage.as_slice());
    assert_eq!(
        hex, GOLDEN_PREIMAGE_HEX,
        "mandate preimage byte layout drifted — this breaks every signed mandate"
    );
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ---------------------------------------------------------------------------
// On-chain mandate signature verification — adversarial matrix
// ---------------------------------------------------------------------------

#[test]
fn happy_path_signature_verifies_at_init() {
    // The deploy helper signs the canonical preimage with the treasury key and
    // supplies the treasury public key. If `init`'s on-chain verification did
    // not pass, deploy would revert and `deploy_with` would panic.
    let fx = deploy_with(U512::zero(), U512::zero());
    assert_eq!(fx.contract.get_status(), cadence_vault::vault::Status::Funded);
    // The stored nonce is the one bound into the verified preimage.
    assert_eq!(fx.contract.get_mandate_nonce(), nonce32());
}

#[test]
fn verify_mandate_view_confirms_stored_limits() {
    let fx = deploy_with(U512::zero(), U512::zero());
    let pk = fx.env.public_key(&fx.treasury);
    let preimage = mandate_message_offchain(
        fx.agent,
        fx.treasury,
        "CSPR",
        "USDC",
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        &venues(),
        &[fx.venue_addr],
        &nonce32(),
    );
    let sig = fx.env.sign_message(&preimage, &fx.treasury);
    assert!(fx.contract.verify_mandate(pk, sig));
}

#[test]
fn verify_mandate_view_rejects_wrong_key() {
    let fx = deploy_with(U512::zero(), U512::zero());
    let agent_pk = fx.env.public_key(&fx.agent);
    let preimage = mandate_message_offchain(
        fx.agent,
        fx.treasury,
        "CSPR",
        "USDC",
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        &venues(),
        &[fx.venue_addr],
        &nonce32(),
    );
    // Even a well-formed signature over the right preimage fails: wrong key.
    let sig = fx.env.sign_message(&preimage, &fx.agent);
    assert!(!fx.contract.verify_mandate(agent_pk, sig));
}

#[test]
fn rejects_forged_signature_at_init() {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let unrelated = Bytes::from(b"not-the-mandate".to_vec());
    let forged = env.sign_message(&unrelated, &treasury);
    let result = try_deploy(
        &env,
        DeployArgs {
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            signer: treasury,
            supplied_pk_account: treasury,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: Some(forged),
        },
    );
    let err = result.map(|_| ()).unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::BadSignature.into());
}

#[test]
fn rejects_wrong_signer_key_hash_mismatch() {
    let env = odra_test::env();
    let agent = env.get_account(1);
    let result = try_deploy(
        &env,
        DeployArgs {
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            signer: agent,
            supplied_pk_account: agent,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: None,
        },
    );
    let err = result.map(|_| ()).unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::NotAuthorizedSigner.into());
}

#[test]
fn rejects_wrong_signer_signed_by_attacker() {
    let env = odra_test::env();
    let attacker = env.get_account(3);
    let result = try_deploy(
        &env,
        DeployArgs {
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            signer: attacker,
            supplied_pk_account: env.get_account(0),
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: None,
        },
    );
    let err = result.map(|_| ()).unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::BadSignature.into());
}

#[test]
fn rejects_tampered_limits_signature_mismatch() {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let result = try_deploy(
        &env,
        DeployArgs {
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            signer: treasury,
            supplied_pk_account: treasury,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL * 2),
            override_signature: None,
        },
    );
    let err = result.map(|_| ()).unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::BadSignature.into());
}

#[test]
fn rejects_replayed_signature_under_different_limits() {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let venue_addr = env.get_account(2);
    let original = mandate_message_offchain(
        env.get_account(1),
        treasury,
        "CSPR",
        "USDC",
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        &venues(),
        &[venue_addr],
        &nonce32(),
    );
    let captured_sig = env.sign_message(&original, &treasury);
    let result = try_deploy(
        &env,
        DeployArgs {
            price_floor: U512::from(1_500_000_000u64),
            price_ceiling: U512::from(2_500_000_000u64),
            signer: treasury,
            supplied_pk_account: treasury,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: Some(captured_sig),
        },
    );
    let err = result.map(|_| ()).unwrap_err();
    assert_eq!(err, cadence_vault::vault::Error::BadSignature.into());
}
