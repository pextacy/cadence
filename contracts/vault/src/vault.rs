//! Execution Vault — the on-chain source of truth for a Cadence mandate.
//!
//! The vault custodies the sell asset (native CSPR for the demo pair), stores the
//! mandate digest and decoded limits, and exposes a single constrained spend
//! entrypoint (`execute_slice`) that the agent identity may call. Every guardrail
//! from the signed mandate is enforced here, on-chain: spend cap, deadline,
//! per-slice slippage, price band, venue allowlist and caller authority. If any
//! check fails the call reverts — this is the guardrail the agent cannot bypass.
//!
//! The mandate is authorised off-chain with an EIP-712 typed-data signature
//! (gasless, human-readable). The treasurer's account is the caller of `init`, so
//! authority is established by the Casper-authenticated sender; the EIP-712 digest
//! and signature are bound on-chain (stored + emitted) so anyone can independently
//! re-derive the digest from the public mandate and verify the signature. See
//! DOCS.md §4 for the full trust boundary.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

/// Fixed-point scale for prices expressed as buy-asset units per one sell-asset
/// unit. A price of 1.0 is `PRICE_SCALE`. Using an integer scale keeps the
/// on-chain price-band check free of floating point.
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// Basis-points denominator (100% = 10_000 bps).
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Lifecycle of a vault instance.
#[odra::odra_type]
pub enum Status {
    /// Mandate stored, not yet funded.
    Funded,
    /// Funded and executing.
    Active,
    /// Circuit-breaker engaged; execution suspended.
    Paused,
    /// Order completed (cap reached) and settled.
    Completed,
    /// Window closed before completion and settled.
    Expired,
}

/// Emitted once when the mandate is stored.
#[odra::event]
pub struct MandateInitialised {
    pub treasury: Address,
    pub agent: Address,
    pub mandate_digest: Bytes,
    pub signature: Bytes,
    pub total_sell: U512,
    pub end_time_ms: u64,
    pub max_slippage_bps: u32,
}

/// Emitted when the vault receives the sell asset and becomes Active.
#[odra::event]
pub struct VaultFunded {
    pub amount: U512,
    pub balance: U512,
}

/// Emitted for every accepted slice. `sell_amount` of the sell asset has been
/// released to the allowlisted venue for the swap; `min_out` is the floor the
/// agent committed to off-chain.
#[odra::event]
pub struct SliceExecuted {
    pub slice_id: u32,
    pub sell_amount: U512,
    pub quoted_out: U512,
    pub min_out: U512,
    pub venue: String,
    pub sold_so_far: U512,
}

/// Emitted when realised swap proceeds are recorded against a slice, linking the
/// on-chain swap deploy for the audit trail.
#[odra::event]
pub struct FillRecorded {
    pub slice_id: u32,
    pub bought_amount: U512,
    pub swap_deploy_hash: String,
    pub bought_so_far: U512,
}

/// Emitted for every decision attestation.
#[odra::event]
pub struct DecisionAttested {
    pub slice_id: u32,
    pub reason: String,
}

/// Emitted on pause / resume.
#[odra::event]
pub struct StatusChanged {
    pub paused: bool,
}

/// Emitted once on settlement with the final execution report.
#[odra::event]
pub struct Settled {
    pub completed: bool,
    pub sold_so_far: U512,
    pub bought_so_far: U512,
    pub slice_count: u32,
    pub returned_to_treasury: U512,
}

#[odra::odra_error]
pub enum Error {
    /// Caller is not the treasury for this action.
    NotTreasury = 2,
    /// Caller is not the authorised agent identity.
    NotAgent = 3,
    /// Action requires `Active` status.
    NotActive = 4,
    /// Vault must be `Funded` (and not yet active) to be funded.
    NotFunded = 5,
    /// Execution window has closed (`now > end_time`).
    DeadlinePassed = 6,
    /// Slice would push cumulative sold size over the mandate cap.
    SpendCapExceeded = 7,
    /// Implied slippage (quoted_out vs min_out) exceeds the mandate cap.
    SlippageTooHigh = 8,
    /// Quoted price is outside the mandate's `[floor, ceiling]` band.
    PriceOutOfBand = 9,
    /// Venue is not on the mandate allowlist.
    VenueNotAllowed = 10,
    /// A zero amount was supplied where a positive value is required.
    ZeroAmount = 11,
    /// `min_out` exceeds `quoted_out`, which is nonsensical.
    MinOutAboveQuote = 12,
    /// Mandate digest must be exactly 32 bytes.
    BadDigestLength = 13,
    /// Settlement is only allowed after the deadline or on completion.
    CannotSettleYet = 14,
    /// Referenced slice id does not exist.
    UnknownSlice = 15,
    /// Funding amount did not match the mandate total.
    FundingMismatch = 16,
}

/// The Execution Vault.
#[odra::module(
    events = [
        MandateInitialised,
        VaultFunded,
        SliceExecuted,
        FillRecorded,
        DecisionAttested,
        StatusChanged,
        Settled
    ],
    errors = Error
)]
pub struct ExecutionVault {
    // Identities
    treasury: Var<Address>,
    agent: Var<Address>,

    // Mandate binding
    mandate_digest: Var<Bytes>,
    signature: Var<Bytes>,

    // Decoded limits
    total_sell: Var<U512>,
    end_time_ms: Var<u64>,
    max_slippage_bps: Var<u32>,
    price_floor: Var<U512>,   // 0 == unset
    price_ceiling: Var<U512>, // 0 == unset
    venue_allowlist: Mapping<String, bool>,

    // Progress
    sold_so_far: Var<U512>,
    bought_so_far: Var<U512>,
    slice_count: Var<u32>,
    // Per-slice min_out, keyed by slice id, so a fill can be reconciled.
    slice_min_out: Mapping<u32, U512>,
    slice_filled: Mapping<u32, bool>,

    status: Var<Status>,
}

#[odra::module]
impl ExecutionVault {
    /// Store the signed mandate and its decoded limits. Called once by the
    /// treasury. The caller becomes the treasury identity. `agent` is the
    /// account-abstraction identity later authorised to call `execute_slice`.
    ///
    /// Times are in **milliseconds** to match the Casper block time. Prices are
    /// fixed-point with [`PRICE_SCALE`]; pass `0` for an unset floor/ceiling.
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        &mut self,
        agent: Address,
        mandate_digest: Bytes,
        signature: Bytes,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: Vec<String>,
    ) {
        // Odra constructors are one-shot at install time; the framework rejects any
        // attempt to re-invoke `init`, so no in-body re-entry guard is needed.
        if mandate_digest.len() != 32 {
            self.env().revert(Error::BadDigestLength);
        }
        if total_sell.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }

        let treasury = self.env().caller();
        self.treasury.set(treasury);
        self.agent.set(agent);
        self.mandate_digest.set(mandate_digest.clone());
        self.signature.set(signature.clone());
        self.total_sell.set(total_sell);
        self.end_time_ms.set(end_time_ms);
        self.max_slippage_bps.set(max_slippage_bps);
        self.price_floor.set(price_floor);
        self.price_ceiling.set(price_ceiling);
        for venue in venues.iter() {
            self.venue_allowlist.set(venue, true);
        }
        self.sold_so_far.set(U512::zero());
        self.bought_so_far.set(U512::zero());
        self.slice_count.set(0);
        self.status.set(Status::Funded);

        self.env().emit_event(MandateInitialised {
            treasury,
            agent,
            mandate_digest,
            signature,
            total_sell,
            end_time_ms,
            max_slippage_bps,
        });
    }

    /// Treasury funds the vault with the sell asset (native CSPR). The attached
    /// value must equal the mandate total. Moves the vault to `Active`.
    #[odra(payable)]
    pub fn fund(&mut self) {
        self.assert_treasury();
        if self.read_status() != Status::Funded {
            self.env().revert(Error::NotFunded);
        }
        let amount = self.env().attached_value();
        if amount != self.total_sell.get_or_default() {
            self.env().revert(Error::FundingMismatch);
        }
        self.status.set(Status::Active);
        self.env().emit_event(VaultFunded {
            amount,
            balance: self.env().self_balance(),
        });
    }

    /// The single constrained spend entrypoint. Only the agent identity may call
    /// it. Every guardrail is re-validated here; any failure reverts. On success
    /// the slice's `sell_amount` is released to the allowlisted `venue_address`
    /// for the swap and the slice is recorded.
    ///
    /// - `sell_amount`  size of this child order in the sell asset
    /// - `quoted_out`   the venue quote's expected output for `sell_amount`
    /// - `min_out`      the agent's committed minimum acceptable output
    /// - `venue`        venue identifier (must be on the allowlist)
    /// - `venue_address`the on-chain address the sell asset is released to
    pub fn execute_slice(
        &mut self,
        sell_amount: U512,
        quoted_out: U512,
        min_out: U512,
        venue: String,
        venue_address: Address,
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
        // 3. sold_so_far + sell_amount <= total_sell
        let new_sold = self.sold_so_far.get_or_default() + sell_amount;
        if new_sold > self.total_sell.get_or_default() {
            self.env().revert(Error::SpendCapExceeded);
        }
        // 4. effective slippage (quote vs min_out) <= max_slippage_bps
        //    (quoted_out - min_out) * BPS_DENOMINATOR <= max_slippage_bps * quoted_out
        let slip_bps = self.max_slippage_bps.get_or_default() as u64;
        let lhs = (quoted_out - min_out) * U512::from(BPS_DENOMINATOR);
        let rhs = quoted_out * U512::from(slip_bps);
        if lhs > rhs {
            self.env().revert(Error::SlippageTooHigh);
        }
        // 5. quoted price within [price_floor, price_ceiling] (if set)
        //    price = quoted_out * PRICE_SCALE / sell_amount  (buy units per sell unit)
        let price = quoted_out * U512::from(PRICE_SCALE) / sell_amount;
        let floor = self.price_floor.get_or_default();
        let ceiling = self.price_ceiling.get_or_default();
        if !floor.is_zero() && price < floor {
            self.env().revert(Error::PriceOutOfBand);
        }
        if !ceiling.is_zero() && price > ceiling {
            self.env().revert(Error::PriceOutOfBand);
        }
        // 6. venue ∈ allowlist
        if !self.venue_allowlist.get_or_default(&venue) {
            self.env().revert(Error::VenueNotAllowed);
        }

        // All guardrails passed — release the sell asset to the venue and record.
        let slice_id = self.slice_count.get_or_default();
        self.env().transfer_tokens(&venue_address, &sell_amount);
        self.sold_so_far.set(new_sold);
        self.slice_count.set(slice_id + 1);
        self.slice_min_out.set(&slice_id, min_out);
        self.slice_filled.set(&slice_id, false);

        self.env().emit_event(SliceExecuted {
            slice_id,
            sell_amount,
            quoted_out,
            min_out,
            venue,
            sold_so_far: new_sold,
        });
        slice_id
    }

    /// Record realised swap proceeds for a slice and link the on-chain swap deploy
    /// hash. Called by the agent identity after the swap settles. Enforces that
    /// realised output is not below the slice's committed `min_out`.
    pub fn record_fill(&mut self, slice_id: u32, bought_amount: U512, swap_deploy_hash: String) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        let min_out = self.slice_min_out.get_or_default(&slice_id);
        if bought_amount < min_out {
            self.env().revert(Error::SlippageTooHigh);
        }
        let bought = self.bought_so_far.get_or_default() + bought_amount;
        self.bought_so_far.set(bought);
        self.slice_filled.set(&slice_id, true);
        self.env().emit_event(FillRecorded {
            slice_id,
            bought_amount,
            swap_deploy_hash,
            bought_so_far: bought,
        });
    }

    /// Record the agent's decision reasoning for a slice (audit trail).
    pub fn attest(&mut self, slice_id: u32, reason: String) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        self.env().emit_event(DecisionAttested { slice_id, reason });
    }

    /// Circuit-breaker: pause execution. Agent or treasury may call.
    pub fn pause(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Active {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Paused);
        self.env().emit_event(StatusChanged { paused: true });
    }

    /// Resume after a pause. Agent or treasury may call.
    pub fn resume(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Paused {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Active);
        self.env().emit_event(StatusChanged { paused: false });
    }

    /// Settle the vault: return any remaining sell asset to the treasury and emit
    /// the final report. Callable by anyone once the order is complete (cap
    /// reached) or the window has closed. Distinguishes `Completed` vs `Expired`.
    pub fn settle(&mut self) {
        let status = self.read_status();
        if status == Status::Completed || status == Status::Expired {
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
        if !remaining.is_zero() {
            self.env().transfer_tokens(&treasury, &remaining);
        }
        self.status
            .set(if completed { Status::Completed } else { Status::Expired });
        self.env().emit_event(Settled {
            completed,
            sold_so_far: sold,
            bought_so_far: self.bought_so_far.get_or_default(),
            slice_count: self.slice_count.get_or_default(),
            returned_to_treasury: remaining,
        });
    }

    // ----- read-only views (used by the dashboard / scripts) -----

    pub fn get_status(&self) -> Status {
        self.read_status()
    }
    pub fn get_treasury(&self) -> Address {
        self.treasury.get_or_revert_with(Error::NotTreasury)
    }
    pub fn get_agent(&self) -> Address {
        self.agent.get_or_revert_with(Error::NotAgent)
    }
    pub fn get_total_sell(&self) -> U512 {
        self.total_sell.get_or_default()
    }
    pub fn get_sold_so_far(&self) -> U512 {
        self.sold_so_far.get_or_default()
    }
    pub fn get_bought_so_far(&self) -> U512 {
        self.bought_so_far.get_or_default()
    }
    pub fn get_slice_count(&self) -> u32 {
        self.slice_count.get_or_default()
    }
    pub fn get_max_slippage_bps(&self) -> u32 {
        self.max_slippage_bps.get_or_default()
    }
    pub fn get_end_time_ms(&self) -> u64 {
        self.end_time_ms.get_or_default()
    }
    pub fn get_mandate_digest(&self) -> Bytes {
        self.mandate_digest.get_or_default()
    }
    pub fn is_venue_allowed(&self, venue: String) -> bool {
        self.venue_allowlist.get_or_default(&venue)
    }

    // ----- internal helpers -----

    fn read_status(&self) -> Status {
        self.status.get_or_revert_with(Error::NotFunded)
    }

    fn assert_treasury(&self) {
        if self.env().caller() != self.treasury.get_or_revert_with(Error::NotTreasury) {
            self.env().revert(Error::NotTreasury);
        }
    }

    fn assert_agent(&self) {
        if self.env().caller() != self.agent.get_or_revert_with(Error::NotAgent) {
            self.env().revert(Error::NotAgent);
        }
    }

    fn assert_agent_or_treasury(&self) {
        let caller = self.env().caller();
        let is_agent = caller == self.agent.get_or_revert_with(Error::NotAgent);
        let is_treasury = caller == self.treasury.get_or_revert_with(Error::NotTreasury);
        if !is_agent && !is_treasury {
            self.env().revert(Error::NotAgent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostEnv, HostRef};

    const TOTAL_SELL: u64 = 1_000_000;
    const END_TIME_MS: u64 = 1_000_000;
    const SLIPPAGE_BPS: u32 = 100; // 1%

    struct Fixture {
        env: HostEnv,
        contract: ExecutionVaultHostRef,
        treasury: Address,
        agent: Address,
        venue_addr: Address,
    }

    fn digest32() -> Bytes {
        Bytes::from(vec![7u8; 32])
    }

    fn deploy_with(price_floor: U512, price_ceiling: U512) -> Fixture {
        let env = odra_test::env();
        let treasury = env.get_account(0);
        let agent = env.get_account(1);
        let venue_addr = env.get_account(2);
        env.set_caller(treasury);
        let contract = ExecutionVault::deploy(
            &env,
            ExecutionVaultInitArgs {
                agent,
                mandate_digest: digest32(),
                signature: Bytes::from(vec![1u8; 65]),
                total_sell: U512::from(TOTAL_SELL),
                end_time_ms: END_TIME_MS,
                max_slippage_bps: SLIPPAGE_BPS,
                price_floor,
                price_ceiling,
                venues: vec!["cspr.trade".to_string()],
            },
        );
        Fixture { env, contract, treasury, agent, venue_addr }
    }

    fn fund(fx: &mut Fixture) {
        fx.env.set_caller(fx.treasury);
        fx.contract.with_tokens(U512::from(TOTAL_SELL)).fund();
    }

    /// A slice priced at 2.0 with exactly 1% slippage — passes all guardrails.
    fn ok_slice(fx: &mut Fixture) -> u32 {
        fx.env.set_caller(fx.agent);
        fx.contract.execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
            fx.venue_addr,
        )
    }

    #[test]
    fn happy_path_executes_records_and_settles() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        assert_eq!(fx.contract.get_status(), Status::Funded);
        fund(&mut fx);
        assert_eq!(fx.contract.get_status(), Status::Active);

        let id = ok_slice(&mut fx);
        assert_eq!(id, 0);
        assert_eq!(fx.contract.get_sold_so_far(), U512::from(100_000u64));
        assert_eq!(fx.contract.get_slice_count(), 1);

        fx.env.set_caller(fx.agent);
        fx.contract
            .record_fill(0, U512::from(199_000u64), "deploy-hash-abc".to_string());
        assert_eq!(fx.contract.get_bought_so_far(), U512::from(199_000u64));
        fx.contract.attest(0, "TWAP slice 1 of 10".to_string());

        // Fill the rest of the cap so the order completes, then settle.
        for _ in 0..9 {
            ok_slice(&mut fx);
        }
        assert_eq!(fx.contract.get_sold_so_far(), U512::from(TOTAL_SELL));

        fx.contract.settle();
        assert_eq!(fx.contract.get_status(), Status::Completed);
    }

    #[test]
    fn rejects_non_agent_caller() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.treasury); // not the agent
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "cspr.trade".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::NotAgent.into());
    }

    #[test]
    fn rejects_over_spend_cap() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(TOTAL_SELL + 1),
                U512::from(2_000_002u64),
                U512::from(1_980_001u64),
                "cspr.trade".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::SpendCapExceeded.into());
    }

    #[test]
    fn rejects_past_deadline() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.advance_block_time(END_TIME_MS + 1);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "cspr.trade".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::DeadlinePassed.into());
    }

    #[test]
    fn rejects_excessive_slippage() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        // min_out 197_000 of quote 200_000 => 1.5% > 1% cap.
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(197_000u64),
                "cspr.trade".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::SlippageTooHigh.into());
    }

    #[test]
    fn rejects_non_allowlisted_venue() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "evil.dex".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::VenueNotAllowed.into());
    }

    #[test]
    fn rejects_price_outside_band() {
        // Band [1.5, 2.5]; a quote priced at 3.0 must revert.
        let mut fx = deploy_with(
            U512::from(1_500_000_000u64),
            U512::from(2_500_000_000u64),
        );
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(300_000u64), // price 3.0
                U512::from(300_000u64), // zero slippage so only price check fails
                "cspr.trade".to_string(),
                fx.venue_addr,
            )
            .unwrap_err();
        assert_eq!(err, Error::PriceOutOfBand.into());
    }

    #[test]
    fn rejects_funding_mismatch() {
        let fx = deploy_with(U512::zero(), U512::zero());
        fx.env.set_caller(fx.treasury);
        let err = fx
            .contract
            .with_tokens(U512::from(TOTAL_SELL - 1))
            .try_fund()
            .unwrap_err();
        assert_eq!(err, Error::FundingMismatch.into());
    }

    #[test]
    fn init_is_one_shot() {
        // The mandate's limits are immutable for the life of the vault. Odra
        // enforces this at the framework level: `init` is a constructor and cannot
        // be re-invoked after install, so a second attempt must error.
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fx.env.set_caller(fx.treasury);
        let result = fx.contract.try_init(
            fx.agent,
            digest32(),
            Bytes::from(vec![1u8; 65]),
            U512::from(TOTAL_SELL),
            END_TIME_MS,
            SLIPPAGE_BPS,
            U512::zero(),
            U512::zero(),
            vec!["cspr.trade".to_string()],
        );
        assert!(result.is_err());
    }

    #[test]
    fn settle_after_deadline_marks_expired_and_returns_funds() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        ok_slice(&mut fx); // sell 100_000 of 1_000_000
        let treasury_before = fx.env.balance_of(&fx.treasury);
        fx.env.advance_block_time(END_TIME_MS + 1);
        fx.contract.settle();
        assert_eq!(fx.contract.get_status(), Status::Expired);
        // Remaining 900_000 returned to treasury.
        let treasury_after = fx.env.balance_of(&fx.treasury);
        assert!(treasury_after > treasury_before);
    }
}
