//! Read-only views used by the dashboard / scripts, plus the public
//! `verify_mandate` re-derivation of the on-chain authorization.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use super::errors::Error;
use super::status::Status;
use super::storage::ExecutionVault;

impl ExecutionVault {
    pub(super) fn get_status_impl(&self) -> Status {
        self.read_status()
    }
    pub(super) fn get_treasury_impl(&self) -> Address {
        self.treasury.get_or_revert_with(Error::NotTreasury)
    }
    pub(super) fn get_agent_impl(&self) -> Address {
        self.agent.get_or_revert_with(Error::NotAgent)
    }
    pub(super) fn get_sell_asset_impl(&self) -> String {
        self.sell_asset.get_or_default()
    }
    pub(super) fn get_buy_asset_impl(&self) -> String {
        self.buy_asset.get_or_default()
    }
    pub(super) fn get_total_sell_impl(&self) -> U512 {
        self.total_sell.get_or_default()
    }
    pub(super) fn get_sold_so_far_impl(&self) -> U512 {
        self.sold_so_far.get_or_default()
    }
    pub(super) fn get_bought_so_far_impl(&self) -> U512 {
        self.bought_so_far.get_or_default()
    }
    pub(super) fn get_slice_count_impl(&self) -> u32 {
        self.slice_count.get_or_default()
    }
    pub(super) fn get_max_slippage_bps_impl(&self) -> u32 {
        self.max_slippage_bps.get_or_default()
    }
    pub(super) fn get_end_time_ms_impl(&self) -> u64 {
        self.end_time_ms.get_or_default()
    }
    pub(super) fn get_mandate_digest_impl(&self) -> Bytes {
        self.mandate_digest.get_or_default()
    }
    pub(super) fn get_mandate_nonce_impl(&self) -> Bytes {
        self.mandate_nonce.get_or_default()
    }
    pub(super) fn is_venue_allowed_impl(&self, venue: String) -> bool {
        self.venue_allowlist.get_or_default(&venue)
    }

    /// Public view that re-runs the stored-limits → preimage → `verify_signature`
    /// check so anyone can independently confirm the on-chain limits match the
    /// treasury's Casper signature without trusting `init`-time emission. Returns
    /// `true` only if `treasury_public_key` hashes to the stored treasury AND the
    /// `signature` verifies against the canonical preimage of the stored limits.
    pub(super) fn verify_mandate_impl(
        &self,
        treasury_public_key: PublicKey,
        signature: Bytes,
    ) -> bool {
        let treasury = match self.treasury.get() {
            Some(t) => t,
            None => return false,
        };
        if Address::from(treasury_public_key.clone()) != treasury {
            return false;
        }
        let agent = match self.agent.get() {
            Some(a) => a,
            None => return false,
        };
        let preimage = self.mandate_message(
            agent,
            treasury,
            &self.sell_asset.get_or_default(),
            &self.buy_asset.get_or_default(),
            self.total_sell.get_or_default(),
            self.end_time_ms.get_or_default(),
            self.max_slippage_bps.get_or_default(),
            self.price_floor.get_or_default(),
            self.price_ceiling.get_or_default(),
            &self.venue_ids.get_or_default(),
            &self.venue_addr_list.get_or_default(),
            &self.mandate_nonce.get_or_default(),
        );
        self.env()
            .verify_signature(&preimage, &signature, &treasury_public_key)
    }
}
