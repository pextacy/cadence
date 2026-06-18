//! Contract error codes for [`crate::token::X402Token`].

use odra::prelude::*;

#[odra::odra_error]
pub enum Error {
    /// Transfer amount exceeds the holder's balance.
    InsufficientBalance = 1,
    /// `transfer_from` amount exceeds the caller's allowance.
    InsufficientAllowance = 2,
    /// Caller is not the token owner (the mint authority).
    NotOwner = 3,
    /// A zero amount was supplied where a positive value is required.
    ZeroAmount = 4,
    /// Arithmetic overflow in supply or balance.
    Overflow = 5,
    /// The supplied public key does not hash to the `from` account.
    NotAuthorizedSigner = 6,
    /// `now < valid_after` — the authorization is not yet usable.
    AuthorizationNotYetValid = 7,
    /// `now > valid_before` — the authorization window has closed.
    AuthorizationExpired = 8,
    /// This `(from, nonce)` authorization has already been used.
    AuthorizationAlreadyUsed = 9,
    /// The signature does not verify against the authorization preimage.
    BadSignature = 10,
    /// Failed to serialize the authorization preimage.
    SerializationError = 11,
}
