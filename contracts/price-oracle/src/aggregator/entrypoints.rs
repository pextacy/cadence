//! The deployable [`OracleAggregator`] contract: storage + entrypoints.
//!
//! Aggregates several independent [`SignedPriceOracle`](crate::signed_oracle)
//! feeds into a single robust quote. `latest_price` cross-calls each configured
//! source via the generated `OracleAdapterContractRef`, drops sources that are
//! unset or stale (older than the aggregator's own `max_staleness_ms`), requires
//! at least `quorum` fresh quotes, and returns the [`median`](super::median) of
//! them.
//!
//! Reading the *raw* per-source price (`get_price`, which never reverts on
//! staleness) rather than the strict `latest_price` is deliberate: it lets the
//! aggregator drop a single stale source without aborting the whole read, and
//! centralises the freshness policy here instead of trusting each source's own
//! staleness bound.

use odra::casper_types::U512;
use odra::prelude::*;
use odra::ContractRef;

use super::errors::AggregatorError;
use super::median::median_u512;
use super::storage::{AggregatorConfigured, AggregatorStorage};
use crate::types::{OracleAdapterContractRef, PriceData};

/// Median-of-N price aggregator over a fixed set of source oracles.
#[odra::module(errors = AggregatorError)]
pub struct OracleAggregator {
    state: SubModule<AggregatorStorage>,
}

#[odra::module]
impl OracleAggregator {
    /// Configure the aggregator with its source oracles, quorum, and the staleness
    /// bound applied to each source quote.
    ///
    /// Reverts unless at least one source is given, `quorum` is positive and does
    /// not exceed the source count, and `max_staleness_ms` is positive.
    pub fn init(&mut self, sources: Vec<Address>, quorum: u32, max_staleness_ms: u64) {
        if self.state.initialised.get_or_default() {
            self.env().revert(AggregatorError::AlreadyInitialised);
        }
        if sources.is_empty() {
            self.env().revert(AggregatorError::NoSources);
        }
        if quorum == 0 {
            self.env().revert(AggregatorError::ZeroQuorum);
        }
        let source_count = sources.len() as u32;
        if quorum > source_count {
            self.env().revert(AggregatorError::QuorumExceedsSources);
        }
        if max_staleness_ms == 0 {
            self.env().revert(AggregatorError::ZeroStaleness);
        }
        for source in sources {
            self.state.sources.push(source);
        }
        self.state.quorum.set(quorum);
        self.state.max_staleness_ms.set(max_staleness_ms);
        self.state.initialised.set(true);
        self.env().emit_event(AggregatorConfigured {
            source_count,
            quorum,
            max_staleness_ms,
        });
    }

    /// Number of configured source oracles.
    pub fn source_count(&self) -> u32 {
        self.state.source_count()
    }

    /// The configured source oracle at `index`, or `None` if out of range.
    pub fn source_at(&self, index: u32) -> Option<Address> {
        self.state.source_at(index)
    }

    /// The configured quorum threshold.
    pub fn quorum(&self) -> u32 {
        self.state.quorum()
    }

    /// The configured per-source staleness bound (ms).
    pub fn max_staleness_ms(&self) -> u64 {
        self.state.max_staleness_ms()
    }

    /// Aggregate the fresh source quotes for `pair` into a single median price.
    ///
    /// Cross-calls every configured source's non-reverting `get_price`, keeps only
    /// quotes that exist and are no older than `max_staleness_ms`, requires at
    /// least `quorum` survivors, and returns their median. The returned
    /// [`PriceData`] carries the median `price`, the freshest surviving
    /// `timestamp_ms`, and the highest surviving `round`.
    ///
    /// Reverts [`AggregatorError::QuorumNotMet`] if fewer than `quorum` fresh
    /// quotes are available.
    pub fn latest_price(&self, pair: String) -> PriceData {
        let now = self.env().get_block_time();
        let max_staleness = self.state.max_staleness_ms.get_or_default();
        let count = self.state.sources.len();

        let mut prices: Vec<U512> = Vec::new();
        let mut newest_ts: u64 = 0;
        let mut highest_round: u64 = 0;

        for index in 0..count {
            let source = match self.state.sources.get(index) {
                Some(addr) => addr,
                None => continue,
            };
            let oracle = OracleAdapterContractRef::new(self.env(), source);
            let quote = match oracle.get_price(pair.clone()) {
                Some(data) => data,
                None => continue,
            };
            // Drop stale: `now` is always >= an accepted timestamp, so the
            // saturating subtraction never underflows.
            let age = now.saturating_sub(quote.timestamp_ms);
            if age > max_staleness {
                continue;
            }
            prices.push(quote.price);
            if quote.timestamp_ms > newest_ts {
                newest_ts = quote.timestamp_ms;
            }
            if quote.round > highest_round {
                highest_round = quote.round;
            }
        }

        let quorum = self.state.quorum.get_or_default();
        if (prices.len() as u32) < quorum {
            self.env().revert(AggregatorError::QuorumNotMet);
        }

        let price = match median_u512(&prices) {
            Some(p) => p,
            // Unreachable: `prices.len() >= quorum >= 1`, and median only returns
            // `None` for an empty slice. Treat any future regression as a quorum
            // failure rather than panicking.
            None => self.env().revert(AggregatorError::QuorumNotMet),
        };
        PriceData {
            price,
            timestamp_ms: newest_ts,
            round: highest_round,
        }
    }
}
