//! The `#[odra::module]` storage layout for the Execution Vault.
//!
//! This declares the on-chain state and wires the event / error metadata. The
//! behaviour (constructor, spend entrypoint, admin, views, preimage) lives in the
//! sibling `impl ExecutionVault` blocks across this module's files; Odra allows
//! multiple `#[odra::module] impl` blocks for one struct in a single crate.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use cadence_access_control::{roles, AccessControl};

use super::events::{
    DecisionAttested, EmergencyWithdrawn, FillRecorded, MandateInitialised, MandateVerified,
    Settled, SliceExecuted, StatusChanged, VaultFunded,
};
use super::errors::Error;
use super::status::Status;

/// The Execution Vault.
#[odra::module(
    events = [
        MandateInitialised,
        MandateVerified,
        VaultFunded,
        SliceExecuted,
        FillRecorded,
        DecisionAttested,
        StatusChanged,
        EmergencyWithdrawn,
        Settled
    ],
    errors = Error
)]
pub struct ExecutionVault {
    // Identities
    pub(super) treasury: Var<Address>,
    pub(super) agent: Var<Address>,
    /// The treasury's Casper public key, captured at `init` after verification, so
    /// the Casper-native mandate signature can be independently re-checked on-chain.
    pub(super) treasury_public_key: Var<PublicKey>,

    // Mandate binding
    pub(super) mandate_digest: Var<Bytes>,
    pub(super) signature: Var<Bytes>,
    /// The Casper-native mandate signature verified on-chain at `init`.
    pub(super) casper_signature: Var<Bytes>,
    /// The consumed mandate nonce (part of the verified preimage).
    pub(super) mandate_nonce: Var<Bytes>,

    // Decoded limits
    pub(super) sell_asset: Var<String>,
    pub(super) buy_asset: Var<String>,
    pub(super) total_sell: Var<U512>,
    pub(super) end_time_ms: Var<u64>,
    pub(super) max_slippage_bps: Var<u32>,
    pub(super) price_floor: Var<U512>,   // 0 == unset
    pub(super) price_ceiling: Var<U512>, // 0 == unset
    pub(super) venue_allowlist: Mapping<String, bool>,
    // The on-chain destination the sell asset is released to for each allowlisted
    // venue. Set once at `init` from the signed mandate; the agent cannot supply or
    // override it, so it can never redirect funds to an address it controls.
    pub(super) venue_addr: Mapping<String, Address>,
    // Whether a venue's destination is a `VenueAdapter` contract the vault settles
    // through cross-contract (true), or a plain destination it transfers to
    // directly (false, the default). Set by the treasury via `set_venue_adapter`.
    pub(super) venue_is_adapter: Mapping<String, bool>,
    // Ordered, canonical copies of the venue id / address lists (Odra `Mapping` is
    // not enumerable). Stored so `verify_mandate` can rebuild the exact preimage.
    pub(super) venue_ids: Var<Vec<String>>,
    pub(super) venue_addr_list: Var<Vec<Address>>,

    // Progress
    pub(super) sold_so_far: Var<U512>,
    pub(super) bought_so_far: Var<U512>,
    pub(super) slice_count: Var<u32>,
    // Per-slice min_out, keyed by slice id, so a fill can be reconciled.
    pub(super) slice_min_out: Mapping<u32, U512>,
    pub(super) slice_filled: Mapping<u32, bool>,

    pub(super) status: Var<Status>,

    /// Role-based access control. Composed (never deployed standalone) so the
    /// vault shares the desk-wide RBAC vocabulary: TREASURY/AGENT/GUARDIAN are
    /// bootstrapped at `init`, and a GUARDIAN (e.g. the desk Guardian contract)
    /// can pause/resume alongside the agent and treasury.
    pub(super) ac: SubModule<AccessControl>,
}

impl ExecutionVault {
    /// Read the lifecycle status, reverting if the vault was never initialised.
    pub(super) fn read_status(&self) -> Status {
        self.status.get_or_revert_with(Error::NotFunded)
    }

    /// Revert unless the caller holds the TREASURY role. Authorization now runs
    /// through the composed AccessControl, but the vault keeps its own error
    /// codes (the role is bootstrapped to the stored treasury at `init`).
    pub(super) fn assert_treasury(&self) {
        if !self.ac.has_role(roles::TREASURY, self.env().caller()) {
            self.env().revert(Error::NotTreasury);
        }
    }

    /// Revert unless the caller holds the AGENT role.
    pub(super) fn assert_agent(&self) {
        if !self.ac.has_role(roles::AGENT, self.env().caller()) {
            self.env().revert(Error::NotAgent);
        }
    }

    /// Revert unless the caller may operate the circuit breaker: the agent, the
    /// treasury, or a GUARDIAN (e.g. the desk-wide Guardian contract).
    pub(super) fn assert_can_pause(&self) {
        let caller = self.env().caller();
        if !self.ac.has_role(roles::AGENT, caller)
            && !self.ac.has_role(roles::TREASURY, caller)
            && !self.ac.has_role(roles::GUARDIAN, caller)
        {
            self.env().revert(Error::NotAgent);
        }
    }
}
