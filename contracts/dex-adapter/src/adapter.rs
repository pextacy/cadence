//! The `VenueAdapter` trait boundary and its `SwapReceipt` return type.
//!
//! `VenueAdapter` is the single cross-contract interface the vault calls to settle
//! a slice. It is declared as an `#[odra::external_contract]` so the vault can build
//! a `VenueAdapterContractRef::new(env, adapter_addr)` from an `Address` it resolved
//! out of the venue registry and call `swap` / `venue_id` without depending on the
//! concrete adapter type.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

/// What an adapter returns from `swap`: the realised buy-asset amount, an opaque
/// on-chain settlement reference (a deploy/transfer hash for off-chain venues, or a
/// pool-fill marker for atomic venues), and whether settlement completed atomically.
///
/// For atomic adapters `atomic == true` and `bought_amount` is final — the vault can
/// record the fill immediately. For escrow adapters `atomic == false`,
/// `bought_amount` is `0` (the realised amount is not yet known on-chain), and the
/// fill is proven later via a signed settlement attestation.
#[odra::odra_type]
pub struct SwapReceipt {
    /// Realised buy-asset amount. Non-zero only for atomic settlement.
    pub bought_amount: U512,
    /// Opaque on-chain settlement reference (deploy/transfer hash or pool marker).
    pub settlement_ref: Bytes,
    /// `true` when the swap settled in this same transaction (atomic on-chain pool);
    /// `false` for two-phase escrow venues awaiting an attested fill.
    pub atomic: bool,
}

/// The venue settlement interface the vault calls cross-contract.
///
/// Implementers: [`crate::cep18_swap::Cep18SwapAdapter`] (atomic on-chain pool) and
/// [`crate::settlement::SettlementAdapter`] (escrow + signed settlement attestation).
///
/// Contract: `swap` MUST revert if a realised, atomically-known output would be
/// below `min_out`. Escrow adapters that cannot know the output atomically defer
/// that check to their attested-fill entrypoint, which enforces `min_out` there.
#[odra::external_contract]
pub trait VenueAdapter {
    /// Settle (or escrow for later settlement) `sell_amount` of `sell_asset` into
    /// `buy_asset`, crediting the realised buy asset to `recipient`.
    ///
    /// MUST revert if `realized_out < min_out` for atomic venues. For escrow venues
    /// this is two-phase: escrow now (returning `atomic = false`), settle on the
    /// later attested fill where `min_out` is enforced.
    fn swap(
        &mut self,
        sell_asset: String,
        buy_asset: String,
        sell_amount: U512,
        min_out: U512,
        recipient: Address,
    ) -> SwapReceipt;

    /// Stable identifier of the venue this adapter settles against (e.g.
    /// `"cspr.trade"` or `"cep18-pool"`). Used by the registry/vault allowlist.
    fn venue_id(&self) -> String;
}
