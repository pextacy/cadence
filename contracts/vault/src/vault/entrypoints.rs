//! The single `#[odra::module] impl` surface for [`ExecutionVault`].
//!
//! Odra 2.8.1 permits exactly one `#[odra::module] impl` block per module (each
//! one generates the full ref / blueprint / schema scaffolding, so a second block
//! collides). This file is therefore the *only* place entrypoints are declared:
//! every method is a thin shim that delegates to the domain logic living in the
//! sibling plain-`impl` blocks (`lifecycle`, `execution`, `admin`, `views`). The
//! split keeps each behavioural file focused while satisfying the framework's
//! one-impl-per-module rule.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use super::status::Status;
use super::storage::ExecutionVault;

#[odra::module]
impl ExecutionVault {
    /// Store the signed mandate and its decoded limits. See
    /// [`ExecutionVault::init_impl`](super::storage::ExecutionVault) in `lifecycle`.
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        &mut self,
        agent: Address,
        mandate_digest: Bytes,
        signature: Bytes,
        treasury_public_key: PublicKey,
        casper_signature: Bytes,
        mandate_nonce: Bytes,
        sell_asset: String,
        buy_asset: String,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: Vec<String>,
        venue_addresses: Vec<Address>,
    ) {
        self.init_impl(
            agent,
            mandate_digest,
            signature,
            treasury_public_key,
            casper_signature,
            mandate_nonce,
            sell_asset,
            buy_asset,
            total_sell,
            end_time_ms,
            max_slippage_bps,
            price_floor,
            price_ceiling,
            venues,
            venue_addresses,
        )
    }

    /// Treasury funds the vault; the attached value must equal the mandate total.
    #[odra(payable)]
    pub fn fund(&mut self) {
        self.fund_impl()
    }

    /// The single constrained spend entrypoint (agent-only).
    pub fn execute_slice(
        &mut self,
        sell_amount: U512,
        quoted_out: U512,
        min_out: U512,
        venue: String,
    ) -> u32 {
        self.execute_slice_impl(sell_amount, quoted_out, min_out, venue)
    }

    /// Record realised swap proceeds for a slice (agent-only).
    pub fn record_fill(&mut self, slice_id: u32, bought_amount: U512, swap_deploy_hash: String) {
        self.record_fill_impl(slice_id, bought_amount, swap_deploy_hash)
    }

    /// Record the agent's decision reasoning for a slice (audit trail).
    pub fn attest(&mut self, slice_id: u32, reason: String) {
        self.attest_impl(slice_id, reason)
    }

    /// Circuit-breaker: pause execution.
    pub fn pause(&mut self) {
        self.pause_impl()
    }

    /// Resume after a pause.
    pub fn resume(&mut self) {
        self.resume_impl()
    }

    /// Emergency drain (treasury-only, requires `Paused`).
    pub fn emergency_withdraw(&mut self) {
        self.emergency_withdraw_impl()
    }

    /// Settle the vault (open-callable after completion or the deadline).
    pub fn settle(&mut self) {
        self.settle_impl()
    }

    /// Treasury wires/rotates the GUARDIAN role (e.g. to the desk Guardian contract).
    pub fn set_guardian(&mut self, guardian: Address) {
        self.set_guardian_impl(guardian)
    }

    /// Whether `who` holds the GUARDIAN role.
    pub fn is_guardian(&self, who: Address) -> bool {
        self.ac.has_role(cadence_access_control::roles::GUARDIAN, who)
    }

    /// Treasury opts a venue into cross-contract `VenueAdapter` settlement (vs the
    /// default direct transfer).
    pub fn set_venue_adapter(&mut self, venue: String, is_adapter: bool) {
        self.set_venue_adapter_impl(venue, is_adapter)
    }

    /// Treasury wires the optional oracle price cross-check (oracle address, the
    /// price-pair key, and the max permitted deviation in bps).
    pub fn set_oracle(&mut self, oracle: Address, pair: String, tolerance_bps: u32) {
        self.set_oracle_impl(oracle, pair, tolerance_bps)
    }

    pub fn get_status(&self) -> Status {
        self.get_status_impl()
    }
    pub fn get_treasury(&self) -> Address {
        self.get_treasury_impl()
    }
    pub fn get_agent(&self) -> Address {
        self.get_agent_impl()
    }
    pub fn get_sell_asset(&self) -> String {
        self.get_sell_asset_impl()
    }
    pub fn get_buy_asset(&self) -> String {
        self.get_buy_asset_impl()
    }
    pub fn get_total_sell(&self) -> U512 {
        self.get_total_sell_impl()
    }
    pub fn get_sold_so_far(&self) -> U512 {
        self.get_sold_so_far_impl()
    }
    pub fn get_bought_so_far(&self) -> U512 {
        self.get_bought_so_far_impl()
    }
    pub fn get_slice_count(&self) -> u32 {
        self.get_slice_count_impl()
    }
    pub fn get_max_slippage_bps(&self) -> u32 {
        self.get_max_slippage_bps_impl()
    }
    pub fn get_end_time_ms(&self) -> u64 {
        self.get_end_time_ms_impl()
    }
    pub fn get_mandate_digest(&self) -> Bytes {
        self.get_mandate_digest_impl()
    }
    pub fn get_mandate_nonce(&self) -> Bytes {
        self.get_mandate_nonce_impl()
    }
    pub fn is_venue_allowed(&self, venue: String) -> bool {
        self.is_venue_allowed_impl(venue)
    }

    /// Independently re-derive and verify the stored mandate authorization.
    pub fn verify_mandate(&self, treasury_public_key: PublicKey, signature: Bytes) -> bool {
        self.verify_mandate_impl(treasury_public_key, signature)
    }
}
