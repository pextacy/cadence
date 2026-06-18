//! The canonical signed-price preimage — **FROZEN BYTE LAYOUT**.
//!
//! The operator signs, and [`SignedPriceOracle`](crate::signed_oracle) verifies,
//! the buffer
//!
//! ```text
//! self_address ‖ pair ‖ price ‖ timestamp_ms ‖ round
//! ```
//!
//! where each field is concatenated in its Casper `ToBytes` encoding. The contract
//! address binds the price to *this* oracle, preventing cross-contract replay. An
//! off-chain signer MUST reproduce this exact buffer (see
//! `mandate/src/settlementAttest.ts` discipline).
//!
//! This field order and encoding is part of the on-chain signature contract: any
//! change invalidates every previously-signed price. Do not reorder, add, remove,
//! or re-encode fields. The golden-vector test below locks the exact bytes.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::prelude::*;

/// Build the canonical price preimage from its component fields.
///
/// Returns `None` if any field fails to serialize (the caller maps this to its
/// `SerializationError` revert). Field order is frozen — see the module docs.
pub fn price_message(
    oracle: &Address,
    pair: &str,
    price: U512,
    timestamp_ms: u64,
    round: u64,
) -> Option<Bytes> {
    let parts: [Result<Vec<u8>, _>; 5] = [
        oracle.to_bytes(),
        pair.to_bytes(),
        price.to_bytes(),
        timestamp_ms.to_bytes(),
        round.to_bytes(),
    ];
    let mut buf: Vec<u8> = Vec::new();
    for part in parts {
        match part {
            Ok(bytes) => buf.extend_from_slice(&bytes),
            Err(_) => return None,
        }
    }
    Some(Bytes::from(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::casper_types::account::AccountHash;

    /// A fixed, deterministic account address for golden-vector stability.
    fn fixed_oracle() -> Address {
        // 32 bytes, ascending — independent of any test env so the vector is stable.
        let mut raw = [0u8; 32];
        for (i, b) in raw.iter_mut().enumerate() {
            *b = i as u8;
        }
        Address::Account(AccountHash::new(raw))
    }

    /// FROZEN golden vector: the exact preimage bytes for a known input. If this
    /// test fails, the signature preimage layout changed and every off-chain
    /// signer and previously-signed price is invalidated. Do NOT "fix" by updating
    /// the expected bytes unless a preimage migration is intended.
    #[test]
    fn price_message_matches_golden_vector() {
        let oracle = fixed_oracle();
        let msg = price_message(&oracle, "CSPR/USDC", U512::from(1_250_000_000u64), 5_000, 1)
            .expect("serialization");

        // Reconstruct the expected buffer field-by-field from the frozen layout.
        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice(&oracle.to_bytes().unwrap());
        expected.extend_from_slice(&"CSPR/USDC".to_bytes().unwrap());
        expected.extend_from_slice(&U512::from(1_250_000_000u64).to_bytes().unwrap());
        expected.extend_from_slice(&5_000u64.to_bytes().unwrap());
        expected.extend_from_slice(&1u64.to_bytes().unwrap());
        assert_eq!(msg.as_slice(), expected.as_slice());

        // Pin the concrete encoding so an accidental ToBytes change is caught even
        // if the reconstruction above drifts in lockstep.
        // Account tag (0x00) + 32 address bytes 0x00..=0x1f.
        assert_eq!(msg[0], 0x00, "address variant tag (Account)");
        assert_eq!(&msg[1..33], &(0u8..32).collect::<Vec<u8>>()[..]);
        // "CSPR/USDC" = 9-byte string: u32 LE length 9 then ASCII.
        assert_eq!(&msg[33..37], &9u32.to_le_bytes());
        assert_eq!(&msg[37..46], b"CSPR/USDC");
    }

    /// The preimage must change if any single field changes (binds all fields).
    #[test]
    fn price_message_is_field_sensitive() {
        let oracle = fixed_oracle();
        let base = price_message(&oracle, "CSPR/USDC", U512::from(100u64), 5_000, 1).unwrap();
        let diff_pair = price_message(&oracle, "BTC/USDC", U512::from(100u64), 5_000, 1).unwrap();
        let diff_price = price_message(&oracle, "CSPR/USDC", U512::from(101u64), 5_000, 1).unwrap();
        let diff_ts = price_message(&oracle, "CSPR/USDC", U512::from(100u64), 5_001, 1).unwrap();
        let diff_round = price_message(&oracle, "CSPR/USDC", U512::from(100u64), 5_000, 2).unwrap();
        assert_ne!(base, diff_pair);
        assert_ne!(base, diff_price);
        assert_ne!(base, diff_ts);
        assert_ne!(base, diff_round);
    }
}
