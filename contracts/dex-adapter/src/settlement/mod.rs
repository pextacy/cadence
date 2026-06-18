//! `SettlementAdapter` — the reference adapter for off-chain MCP venues (cspr.trade).
//!
//! The design findings (and `agent/src/clients/csprTrade.ts`) establish that
//! cspr.trade is an OFF-CHAIN MCP DEX reached over HTTP at `https://mcp.cspr.trade`;
//! it exposes no on-chain router a WASM contract can call atomically. Therefore an
//! atomic on-chain swap against it is infeasible. The production-honest shape is a
//! two-phase escrow + signed-attestation flow:
//!
//! 1. **Escrow (`swap`)** — the vault routes the slice here; the adapter takes
//!    custody of the native sell asset attached to the call, books a per-slice
//!    escrow record, emits a [`SwapIntent`], and returns `atomic = false`. No buy
//!    asset has arrived yet, so `bought_amount` is `0`.
//! 2. **Attested settlement (`record_settlement`)** — after the agent executes the
//!    off-chain swap, the settlement operator signs a canonical preimage over the
//!    realised `bought_amount` and `settlement_ref` and submits it. The adapter
//!    verifies the signature on-chain with `env().verify_signature` against the
//!    registered operator key (the exact x402-token pattern), enforces `min_out`,
//!    replay-protects on `(operator, nonce)`, releases the escrowed sell asset to
//!    the configured sink, and emits [`SettlementRecorded`] — the proof the vault
//!    trusts in place of an unverified `swap_deploy_hash` string.
//!
//! ## Module layout
//!
//! - [`storage`] — the `SettlementAdapter` struct (its on-chain state) and `Escrow`.
//! - [`entrypoints`] — the `swap` / `record_settlement` / view implementations.
//! - [`events`] — `SwapIntent` and `SettlementRecorded`.
//! - [`errors`] — the `Error` revert reasons.
//! - [`preimage`] — the FROZEN canonical signature preimage.

pub mod entrypoints;
pub mod errors;
pub mod events;
pub mod preimage;
pub mod storage;

pub use errors::Error;
pub use events::{SettlementRecorded, SwapIntent};
pub use preimage::settlement_message;
// Glob re-exports so the Odra-generated types reach `settlement::*`:
//  - `storage` holds the struct + the `#[odra::module]`-struct test parts
//    (`SettlementAdapterHostRef`, `SettlementAdapterContractRef`, …).
//  - `entrypoints` holds the `#[odra::module]`-impl deployer parts
//    (`SettlementAdapterInitArgs`, the `Deployer` glue).
pub use entrypoints::*;
pub use storage::*;
