//! `Cep18SwapAdapter` — the reference adapter for venues that ARE on-chain.
//!
//! Unlike cspr.trade (off-chain MCP — see [`crate::settlement`]), an on-chain pool
//! can settle atomically inside the same transaction. This adapter models that case
//! behind the same [`VenueAdapter`](crate::adapter::VenueAdapter) trait: the vault
//! attaches the native sell asset, the adapter prices it against a fixed-point
//! reserve rate, pays the realised buy amount to the recipient from its own native
//! reserve in the same call, and returns `atomic = true` with the realised amount —
//! so the vault records the fill immediately without a later attestation.
//!
//! The price is expressed in [`PRICE_SCALE`] fixed-point units of buy-asset per unit
//! of sell-asset. `bought_amount = sell_amount * price / PRICE_SCALE`. The swap MUST
//! revert when the realised output would fall below `min_out`, satisfying the trait
//! contract for atomic venues.
//!
//! ## Module layout
//!
//! - [`storage`] — the `Cep18SwapAdapter` struct, its `PoolSwap` event, and `Error`.
//! - [`entrypoints`] — the `swap` / owner-admin / view implementations.

pub mod entrypoints;
pub mod storage;

// Glob re-exports so the Odra-generated types reach `cep18_swap::*`:
//  - `storage` holds the struct + struct-macro parts (`Cep18SwapAdapterHostRef`, …).
//  - `entrypoints` holds the impl-macro deployer parts (`Cep18SwapAdapterInitArgs`).
pub use entrypoints::*;
pub use storage::*;

/// Fixed-point scale for the pool price (buy units per sell unit). Re-exported from
/// [`cadence_common::scale::PRICE_SCALE_1E6`] so the on-chain quote math and the
/// shared library agree on the 1e6 DEX scale (value unchanged: `1_000_000`).
pub use cadence_common::scale::PRICE_SCALE_1E6 as PRICE_SCALE;
