//! Revert reasons for the Execution Vault.

use odra::prelude::*;

#[odra::odra_error]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Caller is not the treasury for this action.
    NotTreasury = 2,
    /// Caller is not the authorised agent identity.
    NotAgent = 3,
    /// Action requires `Active` status.
    NotActive = 4,
    /// Vault must be `Funded` (and not yet active) to be funded.
    NotFunded = 5,
    /// Execution window has closed (`now > end_time`).
    DeadlinePassed = 6,
    /// Slice would push cumulative sold size over the mandate cap.
    SpendCapExceeded = 7,
    /// Implied slippage (quoted_out vs min_out) exceeds the mandate cap.
    SlippageTooHigh = 8,
    /// Quoted price is outside the mandate's `[floor, ceiling]` band.
    PriceOutOfBand = 9,
    /// Venue is not on the mandate allowlist.
    VenueNotAllowed = 10,
    /// A zero amount was supplied where a positive value is required.
    ZeroAmount = 11,
    /// `min_out` exceeds `quoted_out`, which is nonsensical.
    MinOutAboveQuote = 12,
    /// Mandate digest must be exactly 32 bytes.
    BadDigestLength = 13,
    /// Settlement is only allowed after the deadline or on completion.
    CannotSettleYet = 14,
    /// Referenced slice id does not exist.
    UnknownSlice = 15,
    /// Funding amount did not match the mandate total.
    FundingMismatch = 16,
    /// The venue list and venue-address list had mismatched lengths at init.
    VenueConfigMismatch = 17,
    /// A fill was already recorded for this slice.
    SliceAlreadyFilled = 18,
    /// The supplied public key does not hash to the `treasury` account.
    NotAuthorizedSigner = 19,
    /// The Casper-native mandate signature does not verify against the preimage.
    BadSignature = 20,
    /// Failed to serialize the mandate preimage.
    SerializationError = 21,
    /// Emergency withdrawal requires the vault to be `Paused`.
    NotPaused = 22,
    /// Arithmetic overflow in a guardrail computation.
    Overflow = 23,
    /// The slice's implied price deviates from the configured oracle price by
    /// more than the allowed band (an extra, dynamic check beyond the static
    /// mandate band; only enforced when an oracle is configured).
    OraclePriceDeviation = 24,
}
