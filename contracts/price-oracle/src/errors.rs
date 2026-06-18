//! Revert errors for the [`SignedPriceOracle`](crate::signed_oracle::SignedPriceOracle).

use odra::prelude::*;

#[odra::odra_error]
pub enum Error {
    /// `init` was called more than once.
    AlreadyInitialised = 1,
    /// The supplied public key does not hash to the registered operator account.
    NotAuthorizedSigner = 2,
    /// The signature does not verify against the price preimage.
    BadSignature = 3,
    /// Failed to serialize the price preimage.
    SerializationError = 4,
    /// `round` did not strictly increase for the pair (stale / replayed update).
    StaleRound = 5,
    /// `timestamp_ms` is older than (or equal to) the stored timestamp.
    StaleTimestamp = 6,
    /// `timestamp_ms` is in the future relative to block time.
    TimestampInFuture = 7,
    /// A zero price was supplied where a positive value is required.
    ZeroPrice = 8,
    /// `latest_price` was requested for a pair with no accepted price yet.
    NoPrice = 9,
    /// The freshest price for the pair is older than `max_staleness_ms`.
    StalePrice = 10,
    /// `max_staleness_ms` was zero at init (a price could never be read).
    ZeroStaleness = 11,
    /// Operator rotation attempted by a non-operator caller.
    NotOperator = 12,
}
