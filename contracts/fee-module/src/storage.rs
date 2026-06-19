//! Storage layout for the [`FeeModule`](crate::fees::FeeModule).
//!
//! The module is intentionally small: a single configurable fee rate, a
//! per-collector accrued-balance ledger, and the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module that
//! gates rate changes. Splitting the storage definition out keeps
//! [`fees`](crate::fees) focused on behaviour.

use crate::events::{FeeAccrued, FeeRateChanged, FeeWithdrawn};
use cadence_access_control::AccessControl;
use odra::casper_types::U512;
use odra::prelude::*;

/// Protocol fee accrual module storage.
///
/// `fee_bps` is the current fee rate in basis points (`25` == 0.25%).
/// `accrued` is the ledger of fees owed to each collector account, denominated
/// in the settled asset's units. `ac` is the shared RBAC sub-module: the
/// [`FEE_COLLECTOR`](cadence_access_control::roles::FEE_COLLECTOR) role both
/// receives accruals and authorises rate changes; the deployer bootstraps as
/// `ROOT_ADMIN` so it can delegate that role.
#[odra::module(
    events = [FeeAccrued, FeeWithdrawn, FeeRateChanged],
    errors = crate::errors::Error
)]
pub struct FeeModule {
    /// Shared RBAC sub-module gating rate changes.
    pub(crate) ac: SubModule<AccessControl>,
    /// Current protocol fee rate, in basis points.
    pub(crate) fee_bps: Var<u32>,
    /// `collector -> accrued balance` ledger, in settled-asset units.
    pub(crate) accrued: Mapping<Address, U512>,
}
