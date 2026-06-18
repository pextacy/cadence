//! Events emitted by the [`Guardian`](crate::guardian::Guardian).
//!
//! Three distinct events, so they live in their own module (a single-event file
//! would be co-located instead): the desk-wide flag flip ([`GlobalPause`]), the
//! per-batch fan-out audit record ([`VaultPauseFanned`]), and guardian rotation
//! ([`GuardianRotated`]).

use odra::prelude::*;

/// Emitted whenever the desk-wide kill switch flips. `paused = true` engages the
/// global pause; `false` lifts it. `by` is the guardian account that triggered it.
#[odra::event]
pub struct GlobalPause {
    /// The new value of the desk-wide pause flag.
    pub paused: bool,
    /// The guardian account that flipped the flag.
    pub by: Address,
}

/// Emitted once per fan-out batch, summarising the cross-contract sweep over a
/// `[start, start + processed)` slice of the registry. `paused` records the
/// direction (pause vs resume) the batch applied.
#[odra::event]
pub struct VaultPauseFanned {
    /// Direction of the fan-out: `true` paused each vault, `false` resumed.
    pub paused: bool,
    /// First registry id the batch addressed.
    pub start: u64,
    /// Number of vault records the batch addressed (may be < the requested limit
    /// when the registry tail is reached).
    pub processed: u64,
    /// Number of vaults the batch actually sent a control call to (records whose
    /// status warranted a transition).
    pub affected: u64,
    /// The guardian account that triggered the fan-out.
    pub by: Address,
}

/// Emitted when guardian authority is rotated to a new account. `previous` is the
/// outgoing holder, `current` the incoming one.
#[odra::event]
pub struct GuardianRotated {
    /// The account that previously held guardian authority.
    pub previous: Address,
    /// The account now holding guardian authority.
    pub current: Address,
}
