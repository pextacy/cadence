//! Administrative entrypoints: circuit-breaker (`pause`/`resume`), the
//! treasury-only `emergency_withdraw` kill-switch, and open-callable `settle`.

use odra::prelude::*;

use cadence_access_control::roles;

use super::errors::Error;
use super::events::{EmergencyWithdrawn, Settled, StatusChanged};
use super::status::Status;
use super::storage::ExecutionVault;

impl ExecutionVault {
    /// Circuit-breaker: pause execution. Agent, treasury, or GUARDIAN may call
    /// (the GUARDIAN role lets the desk-wide Guardian contract pause this vault).
    ///
    /// **Idempotent.** Only an `Active` vault transitions to `Paused` (and emits);
    /// any other status — already `Paused`, or terminal — is a no-op rather than a
    /// revert. This is load-bearing for the desk-wide Guardian fan-out: the
    /// guardian's `VaultControl::pause` is *specified* idempotent so one vault whose
    /// live status has diverged from the registry (e.g. already paused, or
    /// completed between the registry read and the sweep) cannot revert the whole
    /// batch. Authorization (`assert_can_pause`) still runs first, so an unprivileged
    /// caller is always rejected regardless of status.
    pub(super) fn pause_impl(&mut self) {
        self.assert_can_pause();
        if self.read_status() != Status::Active {
            return;
        }
        self.status.set(Status::Paused);
        self.env().emit_event(StatusChanged { paused: true });
    }

    /// Resume after a pause. Agent, treasury, or GUARDIAN may call.
    ///
    /// **Idempotent**, mirroring [`Self::pause_impl`]: only a `Paused` vault returns
    /// to `Active`; any other status is a no-op, so a Guardian `global_resume` sweep
    /// that re-covers an already-active (or terminal) vault never aborts.
    pub(super) fn resume_impl(&mut self) {
        self.assert_can_pause();
        if self.read_status() != Status::Paused {
            return;
        }
        self.status.set(Status::Active);
        self.env().emit_event(StatusChanged { paused: false });
    }

    /// Treasury wires (or rotates) the GUARDIAN role to `guardian` — typically the
    /// desk-wide Guardian contract, so it can pause this vault in a global halt.
    /// Treasury-only; the treasury retains its own GUARDIAN role.
    pub(super) fn set_guardian_impl(&mut self, guardian: Address) {
        self.assert_treasury();
        let by = self.env().caller();
        self.ac.grant_unchecked(roles::GUARDIAN, guardian, by);
    }

    /// Treasury opts an allowlisted venue into cross-contract settlement through
    /// its `VenueAdapter` (the venue's mandate-bound address is then treated as an
    /// adapter contract), or back to a direct transfer. Treasury-only.
    pub(super) fn set_venue_adapter_impl(&mut self, venue: String, is_adapter: bool) {
        self.assert_treasury();
        if !self.venue_allowlist.get_or_default(&venue) {
            self.env().revert(Error::VenueNotAllowed);
        }
        self.venue_is_adapter.set(&venue, is_adapter);
    }

    /// Treasury configures the optional price-oracle cross-check: `execute_slice`
    /// will additionally require each slice's implied price to be within
    /// `max_deviation_bps` of the oracle's price for `pair`. Treasury-only.
    pub(super) fn set_oracle_impl(
        &mut self,
        oracle: Address,
        pair: String,
        max_deviation_bps: u32,
    ) {
        self.assert_treasury();
        self.oracle.set(oracle);
        self.oracle_pair.set(pair);
        self.oracle_max_deviation_bps.set(max_deviation_bps);
    }

    /// Treasury configures optional protocol fee accrual: every recorded fill will
    /// accrue `fee_module`'s current bps fee on the realised buy amount, credited to
    /// `collector` in the module's ledger. Treasury-only. For the accrual to succeed
    /// the `collector` wiring also requires the fee module's admin to have granted
    /// this vault the FEE_COLLECTOR role on `fee_module` (an out-of-band deploy step);
    /// until both are in place fees stay off and fills are unaffected.
    pub(super) fn set_fee_module_impl(&mut self, fee_module: Address, collector: Address) {
        self.assert_treasury();
        self.fee_module.set(fee_module);
        self.fee_collector.set(collector);
    }

    /// Emergency drain. **Treasury only**, and only while the vault is `Paused`.
    ///
    /// This is the incident kill-switch: when the circuit-breaker has paused the
    /// vault, the treasury may sweep the entire remaining balance back to itself and
    /// move the vault to the terminal `Halted` status, blocking any further
    /// execution. Distinct from `settle` (open-callable, only after the deadline or
    /// completion): `emergency_withdraw` lets the treasury recover funds the moment
    /// something goes wrong, without waiting for the window to close. Funds always
    /// return to the stored `treasury` — never to the agent, and never to a
    /// caller-supplied address.
    pub(super) fn emergency_withdraw_impl(&mut self) {
        self.assert_treasury();
        if self.read_status() != Status::Paused {
            self.env().revert(Error::NotPaused);
        }
        let treasury = self.treasury.get_or_revert_with(Error::NotTreasury);
        let remaining = self.env().self_balance();

        // Effects before interaction: mark terminal first, then transfer.
        self.status.set(Status::Halted);
        let sold = self.sold_so_far.get_or_default();
        self.env().emit_event(EmergencyWithdrawn {
            by: treasury,
            returned_to_treasury: remaining,
            sold_so_far: sold,
        });
        if !remaining.is_zero() {
            self.env().transfer_tokens(&treasury, &remaining);
        }
    }

    /// Settle the vault: return any remaining sell asset to the treasury and emit
    /// the final report. Callable by anyone once the order is complete (cap
    /// reached) or the window has closed. Distinguishes `Completed` vs `Expired`.
    ///
    /// Will not run once the vault is terminal (`Completed`/`Expired`/`Halted`); a
    /// halted vault has already returned its funds via `emergency_withdraw`.
    pub(super) fn settle_impl(&mut self) {
        let status = self.read_status();
        if status == Status::Completed || status == Status::Expired || status == Status::Halted {
            self.env().revert(Error::CannotSettleYet);
        }
        let sold = self.sold_so_far.get_or_default();
        let total = self.total_sell.get_or_default();
        let completed = sold >= total;
        let deadline_passed = self.env().get_block_time() > self.end_time_ms.get_or_default();
        if !completed && !deadline_passed {
            self.env().revert(Error::CannotSettleYet);
        }

        let remaining = self.env().self_balance();
        let treasury = self.treasury.get_or_revert_with(Error::NotTreasury);
        // Effects before interaction.
        self.status.set(if completed {
            Status::Completed
        } else {
            Status::Expired
        });
        self.env().emit_event(Settled {
            completed,
            sold_so_far: sold,
            bought_so_far: self.bought_so_far.get_or_default(),
            slice_count: self.slice_count.get_or_default(),
            returned_to_treasury: remaining,
        });
        if !remaining.is_zero() {
            self.env().transfer_tokens(&treasury, &remaining);
        }
    }
}
