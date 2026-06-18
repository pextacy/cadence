//! The `VenueAdapter` trait boundary and its `SwapReceipt` return type.
//!
//! This is the single cross-contract interface the vault calls to settle a slice.
//! The trait itself lives in [`trait_def`] and the return value in [`receipt`];
//! both are re-exported here so callers can `use crate::adapter::{VenueAdapter,
//! SwapReceipt}` without caring about the internal split.

pub mod receipt;
pub mod trait_def;

pub use receipt::SwapReceipt;
// `#[odra::external_contract]` consumes the `VenueAdapter` trait and emits a
// `VenueAdapterContractRef` / `VenueAdapterHostRef` pair in its place; glob-export so
// the cross-contract ref the vault uses is reachable at `adapter::*`.
pub use trait_def::*;
