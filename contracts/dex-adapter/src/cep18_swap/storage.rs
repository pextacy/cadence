//! Persistent state, the single [`PoolSwap`] event, and revert [`Error`]s for the
//! [`Cep18SwapAdapter`]. The event is co-located here rather than split into its own
//! file: it is the module's only event and shares the same storage concern.

use odra::casper_types::U512;
use odra::prelude::*;

/// Emitted on every atomic on-chain swap, recording the realised terms.
#[odra::event]
pub struct PoolSwap {
    pub vault: Address,
    pub sell_amount: U512,
    pub bought_amount: U512,
    pub recipient: Address,
}

#[odra::odra_error]
pub enum Error {
    /// `swap` was called with a zero sell amount.
    ZeroSellAmount = 1,
    /// The native value attached to `swap` did not equal `sell_amount`.
    SwapAmountMismatch = 2,
    /// Pool price has not been configured.
    PriceNotSet = 3,
    /// Realised `bought_amount` is below the caller's `min_out`.
    SlippageTooHigh = 4,
    /// The pool's native reserve cannot cover the realised buy amount.
    InsufficientReserve = 5,
    /// Arithmetic overflow computing the realised output.
    Overflow = 6,
    /// Caller is not the pool owner (administrative entrypoints).
    NotOwner = 7,
}

/// Atomic on-chain swap adapter against a priced native reserve.
#[odra::module(events = [PoolSwap], errors = Error)]
pub struct Cep18SwapAdapter {
    /// Stable venue id (e.g. `"cep18-pool"`).
    pub(super) venue_id: Var<String>,
    /// Pool owner; may set the price and seed/withdraw the reserve.
    pub(super) owner: Var<Address>,
    /// Fixed-point price: buy units per sell unit, scaled by
    /// [`PRICE_SCALE`](super::PRICE_SCALE).
    pub(super) price: Var<U512>,
}
