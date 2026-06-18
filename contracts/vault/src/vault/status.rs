//! Lifecycle status of a vault instance.

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
    /// Emergency drain executed by the treasury while paused; terminal.
    Halted,
}
