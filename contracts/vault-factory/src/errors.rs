//! Error codes raised by the [`VaultFactory`](crate::factory::VaultFactory).

use odra::prelude::*;

/// Failures the factory can revert with.
///
/// Discriminants are stable on-chain identifiers; do not reorder or renumber.
#[odra::odra_error]
pub enum Error {
    /// Caller does not hold [`FACTORY_ADMIN`](cadence_access_control::roles::FACTORY_ADMIN).
    Unauthorized = 1,
    /// A required input (vault address, treasury, agent) was the zero/uninitialised
    /// value or otherwise malformed.
    InvalidInput = 2,
    /// The supplied vault package-hash reference is empty.
    EmptyWasmRef = 3,
    /// The intent id counter overflowed `u64`.
    Overflow = 4,
    /// No intent exists for the supplied id.
    UnknownIntent = 5,
    /// The factory has not been configured with a vault package-hash yet.
    WasmNotConfigured = 6,
}
