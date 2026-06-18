//! `cadence-common` — pure, shared execution math for the Cadence contracts.
//!
//! This crate is a `no_std` library (no Odra module, no deploy artifacts). It
//! extracts the guardrail arithmetic that currently lives inline in the vault so
//! the expansion contracts (factory, fee module, treasury, guardian) can reuse the
//! exact same, audited math instead of re-deriving it.
//!
//! Every function here is pure: it takes values, returns `Result`, and never
//! touches contract storage or the Odra environment. Overflow is always handled
//! via [`checked`] helpers that return [`MathError`] rather than panicking, so the
//! caller decides how to revert.
//!
//! The math is a faithful port of `contracts/vault/src/vault.rs` as of this
//! commit — see each submodule for the line it was extracted from.

#![cfg_attr(not(test), no_std)]
extern crate alloc;

pub mod checked;
pub mod fee;
pub mod price;
pub mod scale;
pub mod slippage;

pub use checked::MathError;
