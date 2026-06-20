//! The constrained spend entrypoint and its companions: `execute_slice`,
//! `record_fill`, `attest`.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;
use odra::ContractRef;

use cadence_access_control::roles;
use cadence_common::checked::checked_mul;
use cadence_common::price::implied_price;
use cadence_common::scale::BPS_DENOMINATOR;
use cadence_dex_adapter::adapter::VenueAdapterContractRef;
use cadence_fee_module::FeeCollectorContractRef;
use cadence_price_oracle::types::OracleAdapterContractRef;

use super::errors::Error;
use super::events::{DecisionAttested, FeesFlushed, FillRecorded, SliceExecuted};
use super::guardrails;
use super::status::Status;
use super::storage::ExecutionVault;

impl ExecutionVault {
    /// The single constrained spend entrypoint. Only the agent identity may call
    /// it. Every guardrail is re-validated here; any failure reverts. On success
    /// the slice's `sell_amount` is released to the allowlisted `venue_address`
    /// for the swap and the slice is recorded.
    ///
    /// - `sell_amount`  size of this child order in the sell asset
    /// - `quoted_out`   the venue quote's expected output for `sell_amount`
    /// - `min_out`      the agent's committed minimum acceptable output
    /// - `venue`        venue identifier (must be on the allowlist); the on-chain
    ///   destination is resolved from the mandate, not the caller
    pub(super) fn execute_slice_impl(
        &mut self,
        sell_amount: U512,
        quoted_out: U512,
        min_out: U512,
        venue: String,
    ) -> u32 {
        self.assert_agent();

        // 1. status == Active
        if self.read_status() != Status::Active {
            self.env().revert(Error::NotActive);
        }
        // 2. now <= end_time
        if self.env().get_block_time() > self.end_time_ms.get_or_default() {
            self.env().revert(Error::DeadlinePassed);
        }
        // sanity on amounts
        if sell_amount.is_zero() || quoted_out.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        if min_out > quoted_out {
            self.env().revert(Error::MinOutAboveQuote);
        }
        // 3. sold_so_far + sell_amount <= total_sell (checked arithmetic).
        let new_sold = match guardrails::add_sold(self.sold_so_far.get_or_default(), sell_amount) {
            Ok(v) => v,
            Err(e) => self.env().revert(e),
        };
        if let Err(e) = guardrails::check_spend_cap(new_sold, self.total_sell.get_or_default()) {
            self.env().revert(e);
        }
        // 4. effective slippage (quote vs min_out) <= max_slippage_bps.
        if let Err(e) = guardrails::check_slice_slippage(
            quoted_out,
            min_out,
            self.max_slippage_bps.get_or_default(),
        ) {
            self.env().revert(e);
        }
        // 5. quoted price within [price_floor, price_ceiling] (if set).
        if let Err(e) = guardrails::check_slice_price(
            quoted_out,
            sell_amount,
            self.price_floor.get_or_default(),
            self.price_ceiling.get_or_default(),
        ) {
            self.env().revert(e);
        }
        // 5b. (optional) implied price within the oracle deviation band — a
        //     dynamic guard layered on top of the static mandate band.
        self.check_oracle_band(quoted_out, sell_amount);

        // 6. venue ∈ allowlist — and resolve its mandate-bound destination. The
        //    caller never supplies the address, so it cannot redirect the funds.
        if !self.venue_allowlist.get_or_default(&venue) {
            self.env().revert(Error::VenueNotAllowed);
        }
        let venue_address = match self.venue_addr.get(&venue) {
            Some(addr) => addr,
            None => self.env().revert(Error::VenueNotAllowed),
        };

        // All guardrails passed. Checks-effects-interactions: record the slice
        // BEFORE releasing funds so the invariant audit is obvious and a future
        // re-entrant venue can never observe stale state.
        let slice_id = self.slice_count.get_or_default();
        self.sold_so_far.set(new_sold);
        self.slice_count.set(slice_id + 1);
        self.slice_min_out.set(&slice_id, min_out);
        self.slice_filled.set(&slice_id, false);

        self.env().emit_event(SliceExecuted {
            slice_id,
            sell_amount,
            quoted_out,
            min_out,
            venue: venue.clone(),
            sold_so_far: new_sold,
        });

        if self.venue_is_adapter.get_or_default(&venue) {
            // Settle cross-contract through the typed VenueAdapter instead of a
            // blind transfer. The vault attaches the sell amount as native value;
            // the adapter credits the realised buy asset to the treasury. Atomic
            // venues return the realised amount in this same call, so the fill is
            // recorded immediately; escrow venues return `atomic = false` and the
            // fill is proven later via the agent's `record_fill`.
            let recipient = self.treasury.get_or_revert_with(Error::NotTreasury);
            let receipt = VenueAdapterContractRef::new(self.env(), venue_address)
                .with_tokens(sell_amount)
                .swap(
                    self.sell_asset.get_or_default(),
                    self.buy_asset.get_or_default(),
                    sell_amount,
                    min_out,
                    recipient,
                );
            if receipt.atomic {
                self.record_atomic_fill(slice_id, receipt.bought_amount, receipt.settlement_ref);
            }
        } else {
            // Legacy / off-chain path: release the sell asset to the mandate-bound
            // destination; the agent reports the realised fill via `record_fill`.
            self.env().transfer_tokens(&venue_address, &sell_amount);
        }
        slice_id
    }

    /// Record a fill that settled atomically inside `execute_slice` (when a
    /// `VenueAdapter` returned `atomic = true`). Mirrors `record_fill_impl`'s
    /// effects but omits the agent assertion — the caller (`execute_slice`) has
    /// already enforced it, and the adapter has already guaranteed
    /// `bought_amount >= min_out`.
    fn record_atomic_fill(&mut self, slice_id: u32, bought_amount: U512, settlement_ref: Bytes) {
        let min_out = self.slice_min_out.get_or_default(&slice_id);
        if let Err(e) = guardrails::check_fill_min_out(bought_amount, min_out) {
            self.env().revert(e);
        }
        let bought =
            match guardrails::add_bought(self.bought_so_far.get_or_default(), bought_amount) {
                Ok(v) => v,
                Err(e) => self.env().revert(e),
            };
        self.bought_so_far.set(bought);
        self.slice_filled.set(&slice_id, true);
        self.env().emit_event(FillRecorded {
            slice_id,
            bought_amount,
            swap_deploy_hash: String::from_utf8(settlement_ref.to_vec()).unwrap_or_default(),
            bought_so_far: bought,
        });
        // Fee accrual is DECOUPLED: only accumulate the obligation locally (no
        // external call), so a fee-module fault can never block recording a fill.
        self.accumulate_fee_if_active(bought_amount);
    }

    /// Record realised swap proceeds for a slice and link the on-chain swap deploy
    /// hash. Called by the agent identity after the swap settles. Enforces that
    /// realised output is not below the slice's committed `min_out`.
    pub(super) fn record_fill_impl(
        &mut self,
        slice_id: u32,
        bought_amount: U512,
        swap_deploy_hash: String,
    ) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        // One fill per slice: a second call would double-count `bought_so_far`.
        if self.slice_filled.get_or_default(&slice_id) {
            self.env().revert(Error::SliceAlreadyFilled);
        }
        let min_out = self.slice_min_out.get_or_default(&slice_id);
        if let Err(e) = guardrails::check_fill_min_out(bought_amount, min_out) {
            self.env().revert(e);
        }
        let bought =
            match guardrails::add_bought(self.bought_so_far.get_or_default(), bought_amount) {
                Ok(v) => v,
                Err(e) => self.env().revert(e),
            };
        self.bought_so_far.set(bought);
        self.slice_filled.set(&slice_id, true);
        self.env().emit_event(FillRecorded {
            slice_id,
            bought_amount,
            swap_deploy_hash,
            bought_so_far: bought,
        });
        // Fee accrual is DECOUPLED from fill recording: this only accumulates the
        // obligation locally (no external call). On the escrow/off-chain path the
        // sell asset has already left the vault in the earlier execute_slice tx, so
        // a fee-module revert here would otherwise permanently brick the fill — the
        // accumulate-then-flush split makes that impossible.
        self.accumulate_fee_if_active(bought_amount);
    }

    /// Accumulate a recorded fill's `bought_amount` into the pending fee base, but
    /// ONLY when fee accrual is active (a fee module was wired via `set_fee_module`
    /// and not since disabled). Purely LOCAL — no external call — so it can never
    /// revert a legitimate fill. A no-op when inactive or the amount is zero, so
    /// every venue/test without a fee module is unaffected. The accumulated base is
    /// pushed to the fee module later, separately, by [`flush_fees_impl`].
    fn accumulate_fee_if_active(&mut self, bought_amount: U512) {
        if bought_amount.is_zero() || !self.fee_active.get_or_default() {
            return;
        }
        let pending = self.pending_fee_base.get_or_default();
        // Mirrors the bought_so_far checked add: a U512 overflow here is
        // astronomically impossible, and if it somehow occurred it is the same
        // failure mode the fill's own accounting already reverts on.
        let updated = match pending.checked_add(bought_amount) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        self.pending_fee_base.set(updated);
    }

    /// Push the accumulated protocol-fee obligation to the wired fee module in a
    /// single cross-contract `accrue_fee` call, then reset the pending base. Callable
    /// by the agent or the treasury.
    ///
    /// This is deliberately a SEPARATE entrypoint from fill recording: it is the
    /// only place the external fee call happens, so a fee-module fault (revoked
    /// collector role, a repointed/buggy module, etc.) can only ever fail THIS call
    /// — leaving `pending_fee_base` intact for a retry — and can never block a fill.
    /// Reverts [`Error::FeeNotActive`] if accrual is disabled and
    /// [`Error::NothingToFlush`] if there is nothing accumulated.
    ///
    /// Checks-effects-interactions: the pending base is zeroed BEFORE the external
    /// call, so a revert rolls the zeroing back (preserving the obligation) and a
    /// re-entrant module can never observe a stale, double-flushable balance.
    pub(super) fn flush_fees_impl(&mut self) {
        let caller = self.env().caller();
        if !self.ac.has_role(roles::AGENT, caller) && !self.ac.has_role(roles::TREASURY, caller) {
            self.env().revert(Error::NotAgent);
        }
        if !self.fee_active.get_or_default() {
            self.env().revert(Error::FeeNotActive);
        }
        let pending = self.pending_fee_base.get_or_default();
        if pending.is_zero() {
            self.env().revert(Error::NothingToFlush);
        }
        let fee_module = self.fee_module.get_or_revert_with(Error::FeeNotActive);
        // Effects before interaction.
        self.pending_fee_base.set(U512::zero());
        let fee = FeeCollectorContractRef::new(self.env(), fee_module)
            .accrue_fee(self.buy_asset.get_or_default(), pending);
        self.env().emit_event(FeesFlushed {
            base_amount: pending,
            fee,
        });
    }

    /// Optional oracle cross-check: when an oracle is configured, require this
    /// slice's implied price to be within `oracle_max_deviation_bps` of the
    /// oracle's price for the configured pair. A no-op when no oracle is set.
    /// Cross-contract read via the `OracleAdapter` trait (the same interface the
    /// `SignedPriceOracle` and `OracleAggregator` expose).
    fn check_oracle_band(&self, quoted_out: U512, sell_amount: U512) {
        let oracle = match self.oracle.get() {
            Some(addr) => addr,
            None => return,
        };
        let max_dev = self.oracle_max_deviation_bps.get_or_default();
        let implied = match implied_price(quoted_out, sell_amount) {
            Ok(v) => v,
            Err(_) => self.env().revert(Error::Overflow),
        };
        let oracle_price = OracleAdapterContractRef::new(self.env(), oracle)
            .latest_price(self.oracle_pair.get_or_default())
            .price;
        // |implied - oracle| * BPS <= oracle * max_dev   (cross-multiplied to
        // avoid division; checked to surface any overflow as a clean revert).
        let diff = if implied > oracle_price {
            implied - oracle_price
        } else {
            oracle_price - implied
        };
        let lhs = match checked_mul(diff, U512::from(BPS_DENOMINATOR)) {
            Ok(v) => v,
            Err(_) => self.env().revert(Error::Overflow),
        };
        let rhs = match checked_mul(oracle_price, U512::from(max_dev)) {
            Ok(v) => v,
            Err(_) => self.env().revert(Error::Overflow),
        };
        if lhs > rhs {
            self.env().revert(Error::OraclePriceDeviation);
        }
    }

    /// Record the agent's decision reasoning for a slice (audit trail).
    pub(super) fn attest_impl(&mut self, slice_id: u32, reason: String) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        self.env().emit_event(DecisionAttested { slice_id, reason });
    }
}
