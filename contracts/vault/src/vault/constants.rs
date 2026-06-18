//! Fixed-point and domain constants for the Execution Vault.
//!
//! The numeric scales are re-exported from [`cadence_common::scale`] so the vault
//! and the shared math library can never drift: `PRICE_SCALE` is the canonical
//! 1e9 alias and `BPS_DENOMINATOR` is the basis-points denominator. The domain
//! tag is vault-specific and stays here.

/// Fixed-point scale for prices expressed as buy-asset units per one sell-asset
/// unit. A price of 1.0 is `PRICE_SCALE`. Using an integer scale keeps the
/// on-chain price-band check free of floating point.
pub use cadence_common::scale::PRICE_SCALE;

/// Basis-points denominator (100% = 10_000 bps).
pub use cadence_common::scale::BPS_DENOMINATOR;

/// Domain tag prefixed to every Casper-native mandate preimage. Binds the
/// authorization to the Cadence mandate scheme (versioned) so a signature can never
/// be replayed under a different scheme version.
pub const MANDATE_DOMAIN_TAG: &[u8] = b"Cadence-Mandate-v1";
