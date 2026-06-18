//! Administrative entrypoints: circuit-breaker (`pause`/`resume`), the
//! treasury-only `emergency_withdraw` kill-switch, and open-callable `settle`.

use odra::prelude::*;

use super::errors::Error;
use super::events::{EmergencyWithdrawn, Settled, StatusChanged};
use super::status::Status;
use super::storage::ExecutionVault;

impl ExecutionVault {
    /// Circuit-breaker: pause execution. Agent or treasury may call.
    pub(super) fn pause_impl(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Active {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Paused);
        self.env().emit_event(StatusChanged { paused: true });
    }

    /// Resume after a pause. Agent or treasury may call.
    pub(super) fn resume_impl(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Paused {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Active);
        self.env().emit_event(StatusChanged { paused: false });
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
