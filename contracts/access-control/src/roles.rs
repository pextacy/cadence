//! Role identifiers for the Cadence RBAC primitive.
//!
//! A [`Role`] is a fixed 32-byte identifier (`[u8; 32]`), chosen so it is a
//! stable, collision-resistant key for an Odra [`Mapping`](odra::Mapping)
//! (`[u8; 32]` implements both `ToBytes` and `FromBytes`). The byte layout of
//! each constant is the ASCII role name, left-aligned and zero-padded to 32
//! bytes, so the on-chain key is human-auditable and reproducible off-chain
//! without a hashing dependency.
//!
//! The role set mirrors the locked production design:
//! `TREASURY`, `AGENT`, `GUARDIAN`, `FEE_COLLECTOR`, `ORACLE_OPERATOR`,
//! `FACTORY_ADMIN`. `GUARDIAN` is a NEW role, distinct from `AGENT`/`TREASURY`.

/// A role identifier — a fixed 32-byte tag used as the map key in
/// [`AccessControl`](crate::AccessControl).
pub type Role = [u8; 32];

/// Build a [`Role`] from a short ASCII name, left-aligned and zero-padded.
///
/// `const`-evaluable so the role constants below are compile-time literals.
/// Panics at compile time if `name` exceeds 32 bytes.
pub const fn role_from_name(name: &[u8]) -> Role {
    // Compile-time guard: a name longer than the tag width is a programmer error.
    assert!(name.len() <= 32, "role name exceeds 32 bytes");
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < name.len() {
        out[i] = name[i];
        i += 1;
    }
    out
}

/// Funds the vault, signs the mandate, receives drained/settled funds.
pub const TREASURY: Role = role_from_name(b"cadence.role.treasury");

/// Executes slices under the mandate guardrails.
pub const AGENT: Role = role_from_name(b"cadence.role.agent");

/// Emergency pause + emergency withdraw. Distinct from agent/treasury.
pub const GUARDIAN: Role = role_from_name(b"cadence.role.guardian");

/// Receives protocol fees charged on realized fills.
pub const FEE_COLLECTOR: Role = role_from_name(b"cadence.role.fee_collector");

/// Signs price-feed attestations consumed by the signed oracle.
pub const ORACLE_OPERATOR: Role = role_from_name(b"cadence.role.oracle_operator");

/// May create vaults via the factory and register them.
pub const FACTORY_ADMIN: Role = role_from_name(b"cadence.role.factory_admin");

/// The bootstrap admin role: the role that administers every other role until
/// re-delegated. Mirrors OpenZeppelin's `DEFAULT_ADMIN_ROLE` (the all-zero
/// identifier). `role_admin[role]` defaults to this when unset, so the account
/// holding `ROOT_ADMIN` can grant any role until a more specific admin is set.
pub const ROOT_ADMIN: Role = [0u8; 32];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names_are_left_aligned_and_padded() {
        assert_eq!(&TREASURY[..21], b"cadence.role.treasury");
        assert_eq!(&TREASURY[21..], &[0u8; 11]);
    }

    #[test]
    fn roles_are_distinct() {
        let all = [
            TREASURY,
            AGENT,
            GUARDIAN,
            FEE_COLLECTOR,
            ORACLE_OPERATOR,
            FACTORY_ADMIN,
            ROOT_ADMIN,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "roles {i} and {j} collide");
                }
            }
        }
    }

    #[test]
    fn root_admin_is_zero() {
        assert_eq!(ROOT_ADMIN, [0u8; 32]);
    }
}
