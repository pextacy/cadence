//! The `FeeModule` contract: configurable basis-points protocol fees with a
//! per-collector accrual ledger.
//!
//! ## Responsibility
//!
//! The module does one thing: it computes a fee on a settled notional using the
//! audited basis-points math in [`cadence_common::fee`], credits it to the fee
//! collector's accrued balance, and lets a collector withdraw what it is owed. It
//! never custodies the underlying asset — the accrued ledger is an accounting
//! claim that the treasury/vault settles out of band.
//!
//! ## Authorisation
//!
//! Rate changes (`set_fee_bps`) and accruals (`accrue_fee`) are gated by the
//! shared [`AccessControl`](cadence_access_control::AccessControl) RBAC
//! sub-module: the caller MUST hold
//! [`roles::FEE_COLLECTOR`](cadence_access_control::roles::FEE_COLLECTOR), the
//! role designated to receive protocol fees. The deployer is bootstrapped as
//! `ROOT_ADMIN` (so it can grant the role) and as a `FEE_COLLECTOR` itself.
//! `withdraw` is callable by any account, but only ever moves that account's own
//! accrued balance. Reads are open.
//!
//! ## Cross-contract surface
//!
//! [`FeeCollector`] is the `#[odra::external_contract]` trait the vault calls to
//! accrue a fee without depending on the concrete module type — it builds a
//! `FeeCollectorContractRef::new(env, fee_module_addr)` from a resolved
//! `Address`.

use crate::errors::Error;
use crate::events::{FeeAccrued, FeeRateChanged, FeeWithdrawn};
use crate::storage::FeeModule;
use cadence_access_control::roles;
use cadence_common::checked::MathError;
use cadence_common::fee::fee_amount;
use odra::casper_types::U512;
use odra::prelude::*;

/// Hard ceiling on the configurable fee rate: 10% (1000 bps). Guards against a
/// fat-fingered or malicious rate that would confiscate fills.
pub const MAX_FEE_BPS: u32 = 1_000;

/// The cross-contract fee-accrual interface the vault calls.
///
/// Declared as an `#[odra::external_contract]` so a caller holding only the fee
/// module's `Address` can accrue a fee via
/// `FeeCollectorContractRef::new(env, addr).accrue_fee(asset, amount)`.
#[odra::external_contract]
pub trait FeeCollector {
    /// Charge the current protocol fee on `amount` of `asset` and credit it to
    /// the caller's accrued balance. Returns the fee charged.
    fn accrue_fee(&mut self, asset: String, amount: U512) -> U512;
}

#[odra::module]
impl FeeModule {
    /// Bootstrap: the deployer becomes `ROOT_ADMIN` (so it can grant the
    /// collector role) and is granted
    /// [`roles::FEE_COLLECTOR`]. The starting fee rate is `init_fee_bps`, which
    /// MUST NOT exceed [`MAX_FEE_BPS`].
    pub fn init(&mut self, init_fee_bps: u32) {
        if init_fee_bps > MAX_FEE_BPS {
            self.env().revert(Error::FeeRateTooHigh);
        }
        let deployer = self.env().caller();
        self.ac
            .grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
        self.ac
            .grant_unchecked(roles::FEE_COLLECTOR, deployer, deployer);
        self.fee_bps.set(init_fee_bps);
    }

    /// Update the protocol fee rate. Caller MUST hold
    /// [`roles::FEE_COLLECTOR`]. Reverts [`Error::FeeRateTooHigh`] if `new_bps`
    /// exceeds [`MAX_FEE_BPS`].
    pub fn set_fee_bps(&mut self, new_bps: u32) {
        let caller = self.env().caller();
        self.assert_collector(caller);
        if new_bps > MAX_FEE_BPS {
            self.env().revert(Error::FeeRateTooHigh);
        }
        let previous_bps = self.fee_bps.get_or_default();
        self.fee_bps.set(new_bps);
        self.env().emit_event(FeeRateChanged {
            previous_bps,
            new_bps,
            sender: caller,
        });
    }

    /// Charge the current fee on `amount` of `asset` and credit it to the
    /// caller's accrued balance. Caller MUST hold [`roles::FEE_COLLECTOR`].
    ///
    /// Reverts [`Error::ZeroAmount`] for a zero notional and [`Error::Overflow`]
    /// if the fee math or the ledger update overflows. Returns the fee charged.
    pub fn accrue_fee(&mut self, asset: String, amount: U512) -> U512 {
        let caller = self.env().caller();
        self.assert_collector(caller);
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }

        let fee_bps = self.fee_bps.get_or_default();
        let fee = match fee_amount(amount, fee_bps) {
            Ok(v) => v,
            Err(e) => self.env().revert(map_math(e)),
        };

        let current = self.accrued.get_or_default(&caller);
        let updated = match current.checked_add(fee) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        self.accrued.set(&caller, updated);

        self.env().emit_event(FeeAccrued {
            asset,
            amount,
            fee,
            fee_bps,
            collector: caller,
        });
        fee
    }

    /// Withdraw the caller's entire accrued balance to `recipient`, zeroing the
    /// ledger entry. Reverts [`Error::NothingAccrued`] if the balance is zero.
    /// Returns the amount withdrawn.
    ///
    /// The module is an accounting ledger only: this records the claim as settled
    /// and emits [`FeeWithdrawn`]; moving the underlying asset is the treasury's
    /// responsibility, keyed off this event.
    pub fn withdraw(&mut self, recipient: Address) -> U512 {
        let caller = self.env().caller();
        let amount = self.accrued.get_or_default(&caller);
        if amount.is_zero() {
            self.env().revert(Error::NothingAccrued);
        }
        self.accrued.set(&caller, U512::zero());
        self.env().emit_event(FeeWithdrawn {
            collector: caller,
            recipient,
            amount,
        });
        amount
    }

    /// Current protocol fee rate, in basis points.
    pub fn fee_bps(&self) -> u32 {
        self.fee_bps.get_or_default()
    }

    /// Accrued, not-yet-withdrawn fee balance owed to `who`.
    pub fn accrued_of(&self, who: Address) -> U512 {
        self.accrued.get_or_default(&who)
    }

    /// Whether `who` holds the fee-collector role
    /// ([`roles::FEE_COLLECTOR`]).
    pub fn is_collector(&self, who: Address) -> bool {
        self.ac.has_role(roles::FEE_COLLECTOR, who)
    }

    /// Grant the fee-collector role to `who`. Caller MUST administer the role
    /// (the deployer holds `ROOT_ADMIN`, its default admin).
    pub fn grant_collector(&mut self, who: Address) {
        self.ac.grant_role(roles::FEE_COLLECTOR, who);
    }

    /// Revoke the fee-collector role from `who`. Caller MUST administer the role.
    pub fn revoke_collector(&mut self, who: Address) {
        self.ac.revoke_role(roles::FEE_COLLECTOR, who);
    }

    // ----- internal helpers (never exposed as entrypoints) -----

    /// Revert [`Error::Unauthorized`] unless `who` holds the collector role.
    fn assert_collector(&self, who: Address) {
        if !self.ac.has_role(roles::FEE_COLLECTOR, who) {
            self.env().revert(Error::Unauthorized);
        }
    }
}

/// Map a raw [`MathError`] from the basis-points fee math onto the module's
/// revert. `fee_amount` can only overflow on multiplication (the denominator is
/// the non-zero `BPS_DENOMINATOR` constant), so every variant collapses to
/// [`Error::Overflow`].
fn map_math(_e: MathError) -> Error {
    Error::Overflow
}
