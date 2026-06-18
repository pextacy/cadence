//! The canonical Casper-native mandate preimage — **FROZEN**.
//!
//! The byte layout produced by [`ExecutionVault::mandate_message`] is signed
//! off-chain by the treasury and verified on-chain at `init`. It MUST be
//! reproduced byte-for-byte off-chain (see `mandate/src/casperAuth.ts`). Any
//! change to the field order, the framing, or the domain-tag handling is a
//! consensus-breaking change to every already-signed mandate. The golden-vector
//! test in `tests/signature.rs` pins the current bytes so accidental drift fails
//! the build.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::prelude::*;

use super::constants::MANDATE_DOMAIN_TAG;
use super::errors::Error;
use super::storage::ExecutionVault;

impl ExecutionVault {
    /// The canonical Casper-native mandate preimage that the treasury signs and the
    /// contract verifies. Mirrors the x402-token `authorization_message` discipline
    /// (token.rs:221-247): a domain tag plus every enforced mandate field in Casper
    /// `ToBytes` encoding. Cross-vault replay is prevented by the unique `nonce`
    /// (the vault address is unknowable at signing time — see the module docs).
    ///
    /// **Field order is frozen** and MUST be reproduced byte-for-byte off-chain
    /// (see `mandate/src/casperAuth.ts`):
    ///   domain_tag ‖ agent ‖ treasury ‖ sell_asset ‖ buy_asset ‖ total_sell ‖
    ///   end_time_ms ‖ max_slippage_bps ‖ price_floor ‖ price_ceiling ‖ venues ‖
    ///   venue_addresses ‖ nonce
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mandate_message(
        &self,
        agent: Address,
        treasury: Address,
        sell_asset: &str,
        buy_asset: &str,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: &[String],
        venue_addresses: &[Address],
        nonce: &Bytes,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        // Domain tag is a fixed prefix (not length-prefixed) — unambiguous because
        // the next field (`agent` Address) is itself self-describing in ToBytes.
        buf.extend_from_slice(MANDATE_DOMAIN_TAG);

        let parts: Vec<Result<Vec<u8>, _>> = vec![
            agent.to_bytes(),
            treasury.to_bytes(),
            sell_asset.to_string().to_bytes(),
            buy_asset.to_string().to_bytes(),
            total_sell.to_bytes(),
            end_time_ms.to_bytes(),
            max_slippage_bps.to_bytes(),
            price_floor.to_bytes(),
            price_ceiling.to_bytes(),
            venues.to_vec().to_bytes(),
            venue_addresses.to_vec().to_bytes(),
            nonce.to_bytes(),
        ];
        for part in parts {
            match part {
                Ok(bytes) => buf.extend_from_slice(&bytes),
                Err(_) => self.env().revert(Error::SerializationError),
            }
        }
        Bytes::from(buf)
    }
}
