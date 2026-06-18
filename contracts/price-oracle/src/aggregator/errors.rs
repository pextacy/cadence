//! Revert errors for the [`OracleAggregator`](super::OracleAggregator).
//!
//! Numbered from 100 to stay disjoint from the signed oracle's `Error` (1..=12),
//! so a combined deployment never sees colliding revert codes.

use odra::prelude::*;

#[odra::odra_error]
pub enum AggregatorError {
    /// `init` was called more than once.
    AlreadyInitialised = 100,
    /// `init` was given no source oracle addresses.
    NoSources = 101,
    /// `quorum` was zero (a price could never be returned).
    ZeroQuorum = 102,
    /// `quorum` exceeded the number of configured sources (unreachable).
    QuorumExceedsSources = 103,
    /// `max_staleness_ms` was zero at init (every quote would be dropped).
    ZeroStaleness = 104,
    /// Fewer fresh source quotes were available than the configured quorum.
    QuorumNotMet = 105,
}
