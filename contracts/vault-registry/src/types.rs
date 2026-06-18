//! Value types stored and returned by the [`VaultRegistry`](crate::registry::VaultRegistry).
//!
//! A [`VaultRecord`] is the on-chain index entry for one deployed Cadence vault:
//! its address, the treasury that owns it, the mandate hash it executes under, the
//! block-time it was registered, and its lifecycle [`VaultStatus`]. Records are
//! immutable except for `status`, which moves through the [`VaultStatus`] state
//! machine via `set_status`.

use odra::prelude::*;

/// Lifecycle state of a registered vault.
///
/// Modelled as an explicit enum so illegal states are unrepresentable and every
/// consumer matches exhaustively. The registry only ever *indexes* vaults — it does
/// not move funds — so the status is an advisory signal for off-chain tooling and
/// for the factory/guardian to reflect a vault's operational phase.
#[odra::odra_type]
pub enum VaultStatus {
    /// Registered and executing under its mandate.
    Active,
    /// Temporarily halted (e.g. guardian pause) but not retired.
    Paused,
    /// Mandate fulfilled or wound down; retained for audit, no longer executing.
    Closed,
}

impl VaultStatus {
    /// Whether a transition from `self` to `next` is permitted.
    ///
    /// `Closed` is terminal: once closed a vault cannot re-open. `Active` and
    /// `Paused` may toggle freely, and either may move to `Closed`.
    pub fn can_transition_to(&self, next: &VaultStatus) -> bool {
        match (self, next) {
            // Terminal: nothing leaves Closed.
            (VaultStatus::Closed, _) => false,
            // No-op transitions are rejected by the caller, not here.
            (VaultStatus::Active, VaultStatus::Paused)
            | (VaultStatus::Active, VaultStatus::Closed)
            | (VaultStatus::Paused, VaultStatus::Active)
            | (VaultStatus::Paused, VaultStatus::Closed) => true,
            // Same-state or any other pairing is not a valid transition.
            (VaultStatus::Active, VaultStatus::Active)
            | (VaultStatus::Paused, VaultStatus::Paused) => false,
        }
    }
}

/// One registry entry: the on-chain index record for a deployed vault.
///
/// All fields except `status` are set once at registration and never mutated;
/// `status` advances through the [`VaultStatus`] machine. `id` is the registry's
/// own dense `u64` key (assigned on registration), distinct from `vault` (the
/// vault contract's address).
#[odra::odra_type]
pub struct VaultRecord {
    /// Dense registry id assigned at registration (the `Mapping` key).
    pub id: u64,
    /// The deployed vault contract's address.
    pub vault: Address,
    /// The treasury that owns / funds this vault (the secondary index key).
    pub treasury: Address,
    /// The 32-byte mandate hash this vault executes under.
    pub mandate_hash: [u8; 32],
    /// Block-time (ms since epoch, per the host env) the vault was registered.
    pub registered_at: u64,
    /// Current lifecycle status.
    pub status: VaultStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_and_paused_toggle() {
        assert!(VaultStatus::Active.can_transition_to(&VaultStatus::Paused));
        assert!(VaultStatus::Paused.can_transition_to(&VaultStatus::Active));
    }

    #[test]
    fn either_may_close() {
        assert!(VaultStatus::Active.can_transition_to(&VaultStatus::Closed));
        assert!(VaultStatus::Paused.can_transition_to(&VaultStatus::Closed));
    }

    #[test]
    fn closed_is_terminal() {
        assert!(!VaultStatus::Closed.can_transition_to(&VaultStatus::Active));
        assert!(!VaultStatus::Closed.can_transition_to(&VaultStatus::Paused));
        assert!(!VaultStatus::Closed.can_transition_to(&VaultStatus::Closed));
    }

    #[test]
    fn no_op_transitions_rejected() {
        assert!(!VaultStatus::Active.can_transition_to(&VaultStatus::Active));
        assert!(!VaultStatus::Paused.can_transition_to(&VaultStatus::Paused));
    }
}
