//! # Cadence Fee Module
//!
//! A configurable basis-points protocol-fee accrual contract. The vault (or any
//! authorised executor) calls [`accrue_fee`](storage::FeeModule::accrue_fee) on
//! each settled fill; the module charges `amount * fee_bps / 10_000` using the
//! audited math in [`cadence_common::fee`] and credits it to the caller's
//! accrued balance. A collector later
//! [`withdraw`](storage::FeeModule::withdraw)s its balance. The module never
//! custodies the underlying asset — it is an accounting ledger of fee claims.
//!
//! ## Layout
//!
//! * [`errors`] — the [`Error`](errors::Error) code set.
//! * [`events`] — [`FeeAccrued`](events::FeeAccrued),
//!   [`FeeWithdrawn`](events::FeeWithdrawn),
//!   [`FeeRateChanged`](events::FeeRateChanged).
//! * [`storage`] — the [`FeeModule`](storage::FeeModule) struct and its storage
//!   layout (fee rate, accrued ledger, RBAC sub-module).
//! * [`fees`] — the module entrypoints and the
//!   [`FeeCollector`](fees::FeeCollector) cross-contract trait.
//!
//! Rate changes and accruals are gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module; the
//! authorised role is
//! [`FEE_COLLECTOR`](cadence_access_control::roles::FEE_COLLECTOR).

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
extern crate alloc;

pub mod errors;
pub mod events;
pub mod fees;
pub mod storage;

pub use errors::Error;
pub use events::{FeeAccrued, FeeRateChanged, FeeWithdrawn};
pub use storage::FeeModule;
// `#[odra::external_contract]` consumes the `FeeCollector` trait and emits a
// `FeeCollectorContractRef` / `FeeCollectorHostRef` pair in its place; glob-export
// so the cross-contract ref a caller uses (and `MAX_FEE_BPS`) is reachable at the
// crate root.
pub use fees::*;
