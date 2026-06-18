//! Execution Vault — the on-chain source of truth for a Cadence mandate.
//!
//! The vault custodies the sell asset (native CSPR for the demo pair), stores the
//! mandate digest and decoded limits, and exposes a single constrained spend
//! entrypoint (`execute_slice`) that the agent identity may call. Every guardrail
//! from the signed mandate is enforced here, on-chain: spend cap, deadline,
//! per-slice slippage, price band, venue allowlist and caller authority. If any
//! check fails the call reverts — this is the guardrail the agent cannot bypass.
//!
//! ## Two-signature authorization model
//!
//! The mandate is authored off-chain with an EIP-712 typed-data signature
//! (gasless, human-readable). That digest is Ethereum-flavoured (keccak256 +
//! secp256k1 *recover*) and a Casper contract **cannot** reproduce it: Odra
//! exposes `env().hash()` (**blake2b**, not keccak256) and
//! `env().verify_signature(message, signature, public_key)` (verify against a
//! *supplied* public key, not ecrecover). See the x402-token doc comment
//! (`contracts/x402-token/src/token.rs`).
//!
//! So the vault keeps the EIP-712 digest as the human-readable treasurer artifact
//! (stored + emitted for off-chain re-derivation) AND verifies a **Casper-native**
//! authorization on-chain. The treasury signs the canonical [`mandate_message`]
//! preimage — a domain tag, every enforced limit, and a unique nonce — with their
//! Casper key. At `init` the vault verifies that the supplied `PublicKey` hashes to
//! the `treasury` caller and that the signature verifies against the reconstructed
//! preimage; otherwise it reverts. This binds the on-chain limits provably to the
//! treasury's signature, closing the gap where the EIP-712 digest was stored but
//! never cryptographically checked. The pattern mirrors the x402-token
//! `transfer_with_authorization` flow (token.rs:176-247).
//!
//! ### Why the preimage binds the *nonce*, not `self_address`
//!
//! x402-token can prefix its preimage with `self_address()` because its
//! authorizations are signed **after** the token contract exists. A Cadence
//! mandate is signed **before** the vault is deployed (the vault's `init` *consumes*
//! the signature), so the vault address is unknowable at signing time — the same
//! chicken-and-egg the EIP-712 domain documents (`mandate/src/schema.ts`). Cross-
//! vault replay is therefore prevented by a unique per-mandate `nonce` baked into
//! the signed preimage (every mandate carries a distinct nonce), exactly as the
//! off-chain EIP-712 path already relies on. The domain tag versions the scheme.
//!
//! ## Module layout
//!
//! The implementation is decomposed across focused files, all contributing to the
//! single `ExecutionVault` module (Odra permits multiple `#[odra::module] impl`
//! blocks for one struct in a crate):
//!
//! - [`constants`] — fixed-point scales (re-exported from `cadence-common`) and the
//!   domain tag.
//! - [`status`] — the lifecycle [`Status`] enum.
//! - [`errors`] — the revert [`Error`] enum.
//! - [`events`] — every emitted event.
//! - [`types`] — internal [`VenueConfig`] helper (does NOT affect the preimage).
//! - [`preimage`] — the FROZEN [`mandate_message`] byte layout.
//! - [`guardrails`] — pure predicates delegating math to `cadence-common`.
//! - [`storage`] — the `#[odra::module]` struct + private auth/status helpers.
//! - [`entrypoints`] — the single `#[odra::module] impl` surface; every entrypoint
//!   delegates to the domain logic below.
//! - [`lifecycle`] — `init` / `fund`.
//! - [`execution`] — `execute_slice` / `record_fill` / `attest`.
//! - [`admin`] — `pause` / `resume` / `emergency_withdraw` / `settle`.
//! - [`views`] — `get_*` views + `verify_mandate`.

pub mod admin;
pub mod constants;
pub mod entrypoints;
pub mod errors;
pub mod events;
pub mod execution;
pub mod guardrails;
pub mod lifecycle;
pub mod preimage;
pub mod status;
pub mod storage;
pub mod types;
pub mod views;

pub use constants::{BPS_DENOMINATOR, MANDATE_DOMAIN_TAG, PRICE_SCALE};
// Re-export the Odra-generated scaffolding (`ExecutionVaultHostRef`,
// `ExecutionVaultInitArgs`, the contract ref, deployer, etc.) produced by the
// single `#[odra::module] impl` in `entrypoints`, so downstream crates and the
// integration tests can reach them at the canonical `vault::*` path.
pub use entrypoints::*;
pub use errors::Error;
pub use events::{
    DecisionAttested, EmergencyWithdrawn, FillRecorded, MandateInitialised, MandateVerified,
    Settled, SliceExecuted, StatusChanged, VaultFunded,
};
pub use status::Status;
pub use storage::ExecutionVault;
pub use types::VenueConfig;
