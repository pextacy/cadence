//! The **FROZEN** authorization preimage for `transfer_with_authorization`.
//!
//! This byte layout is consensus-critical: the off-chain x402 payer signs over
//! exactly these bytes and the contract verifies the signature against them. Any
//! change here breaks every previously signed authorization and every off-chain
//! signer. It is therefore pinned by a golden-vector test (see `tests` below) —
//! do NOT change the field order, the encoding, or the concatenation without a
//! coordinated migration.
//!
//! Layout (each field in Casper `ToBytes` encoding, concatenated in order):
//!
//! ```text
//! contract_address ++ from ++ to ++ value ++ valid_after_ms ++ valid_before_ms ++ nonce
//! ```
//!
//! Binding the contract's own address first prevents cross-contract replay: an
//! authorization signed for one token cannot be settled against another.

use odra::casper_types::bytesrepr::{self, Bytes, ToBytes};
use odra::casper_types::U256;
use odra::prelude::{Address, Vec};

/// Build the canonical authorization preimage.
///
/// Returns `Err` only if a field fails `ToBytes` serialization; callers running
/// on-chain map that to a contract revert.
pub fn authorization_message(
    contract: Address,
    from: Address,
    to: Address,
    value: U256,
    valid_after_ms: u64,
    valid_before_ms: u64,
    nonce: &Bytes,
) -> Result<Bytes, bytesrepr::Error> {
    let parts: [Result<Vec<u8>, bytesrepr::Error>; 7] = [
        contract.to_bytes(),
        from.to_bytes(),
        to.to_bytes(),
        value.to_bytes(),
        valid_after_ms.to_bytes(),
        valid_before_ms.to_bytes(),
        nonce.to_bytes(),
    ];
    let mut buf: Vec<u8> = Vec::new();
    for part in parts {
        buf.extend_from_slice(&part?);
    }
    Ok(Bytes::from(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::casper_types::account::AccountHash;

    /// A deterministic account `Address` from a fixed 32-byte seed.
    fn account(seed: u8) -> Address {
        Address::Account(AccountHash::new([seed; 32]))
    }

    /// GOLDEN VECTOR — pins the exact frozen preimage bytes.
    ///
    /// If this test fails, the authorization byte layout changed, which breaks
    /// every off-chain signer and previously issued authorization. Do not
    /// "fix" the expected bytes without a deliberate, coordinated migration.
    #[test]
    fn authorization_message_golden_vector() {
        let contract = account(0x01);
        let from = account(0x02);
        let to = account(0x03);
        let value = U256::from(25_000u64);
        let valid_after_ms = 1_000u64;
        let valid_before_ms = 1_000_000u64;
        let nonce = Bytes::from(vec![9u8; 32]);

        let msg = authorization_message(
            contract,
            from,
            to,
            value,
            valid_after_ms,
            valid_before_ms,
            &nonce,
        )
        .expect("serialization must succeed");

        // Expected = concatenation of each field's Casper ToBytes encoding.
        let expected: Vec<u8> = [
            contract.to_bytes().unwrap(),
            from.to_bytes().unwrap(),
            to.to_bytes().unwrap(),
            value.to_bytes().unwrap(),
            valid_after_ms.to_bytes().unwrap(),
            valid_before_ms.to_bytes().unwrap(),
            nonce.to_bytes().unwrap(),
        ]
        .concat();

        assert_eq!(msg.as_slice(), expected.as_slice());

        // Pin the concrete byte length and a known prefix so the layout cannot
        // silently drift even if `ToBytes` impls change shape.
        // Account address: 1 (variant tag) + 32 (hash) = 33 bytes.
        // contract + from + to = 99 bytes.
        // U256(25_000) = 1 (len) + 2 (value bytes) = 3 bytes.
        // u64 = 8 bytes each => 16 bytes.
        // nonce Bytes(32) = 4 (u32 len) + 32 = 36 bytes.
        // total = 99 + 3 + 16 + 36 = 154 bytes.
        assert_eq!(msg.len(), 154, "frozen preimage length changed");
        // First byte is the Account address variant tag (0), then the hash seed.
        assert_eq!(msg[0], 0u8);
        assert_eq!(msg[1], 0x01u8);
    }

    /// Distinct contract addresses must yield distinct preimages (anti-replay
    /// binding is part of the frozen contract).
    #[test]
    fn contract_binding_changes_preimage() {
        let from = account(0x02);
        let to = account(0x03);
        let value = U256::from(1u64);
        let nonce = Bytes::from(vec![0u8; 4]);

        let a = authorization_message(account(0x10), from, to, value, 0, 1, &nonce).unwrap();
        let b = authorization_message(account(0x11), from, to, value, 0, 1, &nonce).unwrap();
        assert_ne!(a.as_slice(), b.as_slice());
    }
}
