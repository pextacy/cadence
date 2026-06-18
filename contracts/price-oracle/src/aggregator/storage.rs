//! [`OracleAggregator`] storage: the source oracle set, quorum, and staleness gate.
//!
//! Kept separate from the entrypoint logic so the on-chain layout is auditable in
//! one place. Sources are an append-only [`List`] of oracle `Address`es; each must
//! implement [`OracleAdapter`](crate::types::OracleAdapter).

use odra::prelude::*;

use super::errors::AggregatorError;

/// Emitted at `init` recording the configured source set and quorum. A single
/// event, co-located with the storage it describes (no standalone events file).
#[odra::event]
pub struct AggregatorConfigured {
    pub source_count: u32,
    pub quorum: u32,
    pub max_staleness_ms: u64,
}

/// Stored configuration for the aggregator.
#[odra::module(events = [AggregatorConfigured], errors = AggregatorError)]
pub struct AggregatorStorage {
    /// The configured source oracle addresses (each an `OracleAdapter`).
    pub sources: List<Address>,
    /// Minimum number of fresh source quotes required to return a price.
    pub quorum: Var<u32>,
    /// Maximum age (ms) a source quote may have before the aggregator drops it.
    pub max_staleness_ms: Var<u64>,
    /// Set once `init` runs, to make re-initialisation revert.
    pub initialised: Var<bool>,
}

#[odra::module]
impl AggregatorStorage {
    /// Number of configured source oracles.
    pub fn source_count(&self) -> u32 {
        self.sources.len()
    }

    /// The configured source at `index`, or `None` if out of range.
    pub fn source_at(&self, index: u32) -> Option<Address> {
        self.sources.get(index)
    }

    /// The configured quorum threshold.
    pub fn quorum(&self) -> u32 {
        self.quorum.get_or_default()
    }

    /// The configured per-source staleness bound (ms).
    pub fn max_staleness_ms(&self) -> u64 {
        self.max_staleness_ms.get_or_default()
    }
}
