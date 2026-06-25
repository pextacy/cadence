//! Revert reasons for the [`SettlementAdapter`](super::SettlementAdapter).

use odra::prelude::*;

#[odra::odra_error]
pub enum Error {
    /// `swap` was called with a zero sell amount.
    ZeroSellAmount = 1,
    /// The native value attached to `swap` did not equal `sell_amount`.
    EscrowAmountMismatch = 2,
    /// `record_settlement` referenced an unknown escrow id.
    UnknownEscrow = 3,
    /// The escrow has already been settled — one settlement per escrow.
    EscrowAlreadySettled = 4,
    /// Realised `bought_amount` is below the escrow's committed `min_out`.
    SlippageTooHigh = 5,
    /// The supplied public key does not hash to the registered operator account.
    NotAuthorizedSigner = 6,
    /// This `(operator, nonce)` attestation has already been used.
    AttestationAlreadyUsed = 7,
    /// The signature does not verify against the settlement preimage.
    BadSignature = 8,
    /// Failed to serialize the settlement preimage.
    SerializationError = 9,
    /// Caller is not the configured operator (administrative entrypoints).
    NotOperator = 10,
    /// `cancel_escrow` was called by an account other than the escrow's recipient.
    NotRefundRecipient = 11,
    /// `cancel_escrow` was called before the refund timeout elapsed.
    RefundTimeoutNotReached = 12,
    /// The escrow has already been refunded — one terminal outcome per escrow.
    EscrowAlreadyRefunded = 13,
}
