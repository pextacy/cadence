#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
//! # cadence_dex_adapter
//!
//! The `VenueAdapter` trait boundary the `ExecutionVault` calls to settle a swap
//! and obtain on-chain proof of the realised buy-asset amount. This replaces the
//! vault's previous blind `transfer_tokens` to a venue address (vault.rs:357) with
//! a typed, cross-contract settlement interface that MUST revert when realised
//! output falls below `min_out`.
//!
//! ## Two settlement shapes behind one trait
//!
//! Casper venues come in two flavours and the vault must stay agnostic to which one
//! it routes to:
//!
//! 1. **Atomic on-chain pools** — a swap that settles in the same transaction.
//!    Modelled by [`cep18_swap::Cep18SwapAdapter`], which swaps the escrowed sell
//!    asset against an on-chain reserve and returns `atomic = true` with the
//!    realised amount immediately.
//!
//! 2. **Off-chain MCP venues (cspr.trade)** — the design findings confirm
//!    `agent/src/clients/csprTrade.ts` talks to the cspr.trade MCP over HTTP
//!    (`https://mcp.cspr.trade`); cspr.trade is NOT an on-chain router a WASM
//!    contract can call atomically. The production-honest shape is a two-phase
//!    **escrow + attested settlement**, modelled by [`settlement::SettlementAdapter`]:
//!    `swap` escrows the slice and emits a [`settlement::SwapIntent`] (returns
//!    `atomic = false`); the agent performs the off-chain swap; then a settlement
//!    operator proves the realised fill with a Casper-native signature verified
//!    on-chain via `env().verify_signature` — the exact pattern proven in
//!    `x402-token` (`token.rs:205-247`). This replaces the unproven string
//!    `swap_deploy_hash` (vault.rs:377) with a cryptographically checked attestation.
//!
//! Both implementations expose the same `swap` / `venue_id` entrypoints the
//! `adapter::VenueAdapter` external-contract trait declares, so the vault resolves
//! an adapter `Address` from the on-chain venue registry and calls it via the
//! generated `VenueAdapterContractRef`.

extern crate alloc;

pub mod adapter;
pub mod cep18_swap;
pub mod settlement;

pub use adapter::SwapReceipt;
