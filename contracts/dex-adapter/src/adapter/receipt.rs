//! [`SwapReceipt`] — the value an adapter returns from `swap`.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;

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
