//! The [`OracleAggregator`] contract — median-of-N over several signed oracles.
//!
//! Layout mirrors the rest of the crate: [`storage`] holds the on-chain state (and
//! its single config event), [`errors`] the revert codes, [`median`] the pure
//! median rule, and [`entrypoints`] the deployable module wiring them together.

pub mod entrypoints;
pub mod errors;
pub mod median;
pub mod storage;

pub use entrypoints::OracleAggregator;
pub use errors::AggregatorError;
