//! FROZEN: the canonical settlement preimage the operator signs and the contract
//! verifies. The exact byte layout here is consensus-critical — any signature
//! produced off-chain by a settlement operator must reproduce these bytes verbatim,
//! and a live deployment's verifying key was issued against this encoding. DO NOT
//! reorder fields, change the encoding, or alter the concatenation: doing so would
//! silently invalidate every existing operator attestation.
//!
//! The preimage is prefixed with the adapter's own address to bind the attestation
//! to *this* adapter (cross-contract replay protection), then every settlement field
//! in Casper `ToBytes` encoding — the same discipline as `x402-token` (`token.rs`).
//!
//! A golden-vector test (`tests/settlement.rs`) pins the exact bytes for a fixed
//! input so this layout cannot drift unnoticed.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::prelude::*;

/// Outcome of building the preimage: either the canonical bytes, or a serialization
/// failure the caller maps onto its own revert. Kept as a pure `Result` so this
/// module never touches the Odra environment and stays unit-testable.
pub type PreimageResult = Result<Bytes, PreimageError>;

/// A field failed Casper `ToBytes` serialization while assembling the preimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreimageError;

/// Build the canonical, FROZEN settlement preimage.
///
/// Layout (concatenated, in order): `adapter_address || escrow_id || bought_amount
/// || settlement_ref || nonce || recipient`, each field in Casper `ToBytes` form.
pub fn settlement_message(
    adapter_address: &Address,
    escrow_id: u64,
    bought_amount: U512,
    settlement_ref: &Bytes,
    nonce: &Bytes,
    recipient: &Address,
) -> PreimageResult {
    let parts: [Result<Vec<u8>, _>; 6] = [
        adapter_address.to_bytes(),
        escrow_id.to_bytes(),
        bought_amount.to_bytes(),
        settlement_ref.to_bytes(),
        nonce.to_bytes(),
        recipient.to_bytes(),
    ];
    let mut buf: Vec<u8> = Vec::new();
    for part in parts {
        match part {
            Ok(bytes) => buf.extend_from_slice(&bytes),
            Err(_) => return Err(PreimageError),
        }
    }
    Ok(Bytes::from(buf))
}
