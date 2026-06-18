//! Internal value types for the vault.
//!
//! [`VenueConfig`] pairs a venue identifier with the on-chain destination the sell
//! asset is released to for that venue. It is purely an *internal* convenience for
//! reasoning about a venue and its address together.
//!
//! CRITICAL: the signed mandate preimage and on-chain storage still encode the
//! venue list and venue-address list as two **parallel** `Vec<String>` /
//! `Vec<Address>` (see [`super::preimage`]). [`VenueConfig`] never participates in
//! `ToBytes` serialization — [`split`](VenueConfig::split) and
//! [`zip`](VenueConfig::zip) only re-pack the same two lists, so the byte framing
//! of the preimage is identical to before this type existed.

use odra::prelude::*;

/// A single allowlisted venue and the mandate-bound address its swaps settle to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VenueConfig {
    /// Venue identifier (e.g. `"cspr.trade"`).
    pub id: String,
    /// The on-chain destination the sell asset is released to for this venue.
    pub address: Address,
}

impl VenueConfig {
    /// Pair two parallel lists into [`VenueConfig`] values. The caller must have
    /// already verified the lengths match; the shorter length wins via `zip` so no
    /// panic can occur.
    pub fn zip(ids: &[String], addresses: &[Address]) -> Vec<VenueConfig> {
        ids.iter()
            .zip(addresses.iter())
            .map(|(id, address)| VenueConfig {
                id: id.clone(),
                address: *address,
            })
            .collect()
    }

    /// Split a list of configs back into the two parallel lists the preimage and
    /// storage use. The ordering is preserved, so the resulting lists serialize to
    /// the same bytes as the originals.
    pub fn split(configs: &[VenueConfig]) -> (Vec<String>, Vec<Address>) {
        let mut ids = Vec::with_capacity(configs.len());
        let mut addresses = Vec::with_capacity(configs.len());
        for cfg in configs {
            ids.push(cfg.id.clone());
            addresses.push(cfg.address);
        }
        (ids, addresses)
    }
}
