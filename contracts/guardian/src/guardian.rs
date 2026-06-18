//! The `Guardian` contract: the desk-wide kill switch.
//!
//! ## Responsibility
//!
//! The guardian holds the desk-wide pause flag and the address of the
//! [`VaultRegistry`](cadence_vault_registry::registry::VaultRegistry). On an
//! incident a `GUARDIAN`-role holder calls [`Guardian::global_pause`], which
//! flips the desk-wide flag and fans out [`VaultControl::pause`] to every active
//! registered vault. [`Guardian::global_resume`] is the inverse.
//!
//! ## Bounded fan-out
//!
//! A desk can register an unbounded number of vaults, but a single transaction
//! must never sweep an unbounded set (out-of-gas). Both `global_pause` and
//! `global_resume` are **paginated**: the caller supplies `[start, limit)` and
//! `limit` is capped at [`MAX_FANOUT_PER_CALL`]. The desk-wide flag flips on the
//! first page (`start == 0`); later pages continue the sweep and require the flag
//! to already be in the target state. This makes the kill switch resolve in a
//! bounded number of bounded calls.
//!
//! ## Tolerating already-paused vaults
//!
//! [`VaultControl::pause`]/`resume` are specified idempotent (a no-op, not a
//! revert, when the vault is already in the target state), so a sweep that
//! re-covers a vault an earlier batch — or the vault's own agent — already paused
//! does not abort. The guardian additionally only fans out to records the
//! registry reports as `Active` (for pause) or `Paused` (for resume), so the
//! common case never issues a redundant call at all.
//!
//! ## Authorisation
//!
//! Every state-changing entrypoint is gated by the shared
//! [`AccessControl`](cadence_access_control::AccessControl) RBAC sub-module on the
//! [`GUARDIAN`](cadence_access_control::roles::GUARDIAN) role. The deployer is
//! bootstrapped as `ROOT_ADMIN` (so it can rotate guardians) and `GUARDIAN`.

use crate::errors::{Error, MAX_FANOUT_PER_CALL};
use crate::events::{GlobalPause, GuardianRotated, VaultPauseFanned};
use crate::storage::{VaultControlContractRef, VaultRegistryViewContractRef};
use cadence_access_control::roles;
use cadence_access_control::AccessControl;
use cadence_vault_registry::types::{VaultRecord, VaultStatus};
use odra::prelude::*;
use odra::ContractRef;

/// Desk-wide kill switch over every registered Cadence vault.
#[odra::module(
    events = [GlobalPause, VaultPauseFanned, GuardianRotated],
    errors = Error
)]
pub struct Guardian {
    /// Shared RBAC sub-module gating every state-changing entrypoint.
    ac: SubModule<AccessControl>,
    /// Address of the vault registry the guardian enumerates over.
    registry: Var<Address>,
    /// The desk-wide pause flag. `true` once a `global_pause` sweep has begun.
    paused: Var<bool>,
}

#[odra::module]
impl Guardian {
    /// Bootstrap: store the `registry` address and grant the deployer
    /// [`roles::ROOT_ADMIN`] (so it can rotate guardians) and
    /// [`roles::GUARDIAN`].
    pub fn init(&mut self, registry: Address) {
        let deployer = self.env().caller();
        self.registry.set(registry);
        self.paused.set(false);
        self.ac.grant_unchecked(roles::ROOT_ADMIN, deployer, deployer);
        self.ac.grant_unchecked(roles::GUARDIAN, deployer, deployer);
    }

    /// Engage the desk-wide kill switch over the registry slice `[start, limit)`.
    ///
    /// Caller MUST hold [`roles::GUARDIAN`]. On the first page (`start == 0`) the
    /// desk-wide flag flips to paused; reverts [`Error::AlreadyInState`] if it was
    /// already paused. Later pages (`start > 0`) require the flag to already be
    /// paused and just continue the fan-out. Fans out [`VaultControl::pause`] to
    /// every `Active` vault in the slice (idempotent on the vault side). Returns
    /// the number of vaults the batch sent a pause call to.
    pub fn global_pause(&mut self, start: u64, limit: u64) -> u64 {
        self.assert_guardian();
        self.set_flag_for_page(true, start);
        let (processed, affected) = self.fan_out(true, start, limit);
        self.emit_fanned(true, start, processed, affected);
        affected
    }

    /// Lift the desk-wide kill switch over the registry slice `[start, limit)`.
    ///
    /// The resume mirror of [`Guardian::global_pause`]: on the first page the flag
    /// flips to active (reverts [`Error::AlreadyInState`] if already active), and
    /// the batch fans out [`VaultControl::resume`] to every `Paused` vault in the
    /// slice. Returns the number of vaults the batch sent a resume call to.
    pub fn global_resume(&mut self, start: u64, limit: u64) -> u64 {
        self.assert_guardian();
        self.set_flag_for_page(false, start);
        let (processed, affected) = self.fan_out(false, start, limit);
        self.emit_fanned(false, start, processed, affected);
        affected
    }

    /// Rotate guardian authority from the caller to `new_guardian`.
    ///
    /// Caller MUST currently hold [`roles::GUARDIAN`]. Grants the role to
    /// `new_guardian` and revokes it from the caller, then emits
    /// [`GuardianRotated`]. Rotating to oneself is rejected
    /// ([`Error::AlreadyInState`]) since it would be a no-op that drops the role.
    pub fn rotate_guardian(&mut self, new_guardian: Address) {
        let caller = self.env().caller();
        self.assert_guardian();
        if new_guardian == caller {
            self.env().revert(Error::AlreadyInState);
        }
        self.ac.grant_unchecked(roles::GUARDIAN, new_guardian, caller);
        self.revoke_guardian(caller);
        self.env().emit_event(GuardianRotated {
            previous: caller,
            current: new_guardian,
        });
    }

    // ----- read-only views -----

    /// Whether the desk-wide kill switch is currently engaged.
    pub fn is_paused(&self) -> bool {
        self.paused.get_or_default()
    }

    /// The registry address the guardian enumerates over.
    pub fn registry(&self) -> Address {
        self.registry.get_or_revert_with(Error::Unauthorized)
    }

    /// Whether `who` holds the [`roles::GUARDIAN`] role.
    pub fn is_guardian(&self, who: Address) -> bool {
        self.ac.has_role(roles::GUARDIAN, who)
    }

    /// The largest `limit` a single fan-out call accepts ([`MAX_FANOUT_PER_CALL`]).
    pub fn max_fanout_per_call(&self) -> u64 {
        MAX_FANOUT_PER_CALL
    }

    // ----- internal helpers (never exposed as entrypoints) -----

    /// Flip the desk-wide flag for the first page, or validate continuation pages.
    ///
    /// On `start == 0` the flag must move to `target`; a redundant flip reverts
    /// [`Error::AlreadyInState`]. On `start > 0` the flag must already equal
    /// `target` (the sweep is mid-flight) — otherwise the continuation page is out
    /// of order and reverts [`Error::AlreadyInState`].
    fn set_flag_for_page(&mut self, target: bool, start: u64) {
        let current = self.paused.get_or_default();
        if start == 0 {
            if current == target {
                self.env().revert(Error::AlreadyInState);
            }
            self.paused.set(target);
            self.env().emit_event(GlobalPause {
                paused: target,
                by: self.env().caller(),
            });
        } else if current != target {
            self.env().revert(Error::AlreadyInState);
        }
    }

    /// Enumerate the registry slice `[start, limit)` and fan out the control call.
    ///
    /// Returns `(processed, affected)`: how many records the slice covered and how
    /// many warranted (and received) a control call. `limit` is validated against
    /// [`MAX_FANOUT_PER_CALL`] so the cross-contract sweep is always bounded.
    fn fan_out(&self, pausing: bool, start: u64, limit: u64) -> (u64, u64) {
        if limit == 0 || limit > MAX_FANOUT_PER_CALL {
            self.env().revert(Error::InvalidBatchBound);
        }
        let registry = self.registry.get_or_revert_with(Error::Unauthorized);
        let records: Vec<VaultRecord> =
            VaultRegistryViewContractRef::new(self.env(), registry).enumerate(start, limit);

        let processed = records.len() as u64;
        let mut affected: u64 = 0;
        for record in records.iter() {
            if Self::should_act(pausing, &record.status) {
                let mut vault = VaultControlContractRef::new(self.env(), record.vault);
                if pausing {
                    vault.pause();
                } else {
                    vault.resume();
                }
                affected += 1;
            }
        }
        (processed, affected)
    }

    /// Whether a record in `status` warrants a control call in this direction.
    ///
    /// Pausing acts only on `Active` vaults; resuming acts only on `Paused`
    /// vaults. `Closed` vaults are terminal and always skipped. Matching
    /// exhaustively keeps the policy auditable.
    fn should_act(pausing: bool, status: &VaultStatus) -> bool {
        match (pausing, status) {
            (true, VaultStatus::Active) => true,
            (false, VaultStatus::Paused) => true,
            (_, VaultStatus::Active)
            | (_, VaultStatus::Paused)
            | (_, VaultStatus::Closed) => false,
        }
    }

    fn emit_fanned(&self, paused: bool, start: u64, processed: u64, affected: u64) {
        self.env().emit_event(VaultPauseFanned {
            paused,
            start,
            processed,
            affected,
            by: self.env().caller(),
        });
    }

    fn revoke_guardian(&mut self, who: Address) {
        // grant_unchecked / a direct map clear is not exposed; route through the
        // RBAC primitive's revoke, which the caller (a GUARDIAN that also holds
        // ROOT_ADMIN via bootstrap, or whose role admin is ROOT_ADMIN) administers.
        self.ac.revoke_role(roles::GUARDIAN, who);
    }

    /// Revert [`Error::Unauthorized`] unless the caller holds [`roles::GUARDIAN`].
    fn assert_guardian(&self) {
        let caller = self.env().caller();
        if !self.ac.has_role(roles::GUARDIAN, caller) {
            self.env().revert(Error::Unauthorized);
        }
    }
}
