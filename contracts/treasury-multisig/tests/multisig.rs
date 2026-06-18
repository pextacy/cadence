//! Integration tests for the `TreasuryMultisig` contract.

use cadence_treasury_multisig::errors::Error;
use cadence_treasury_multisig::multisig::{TreasuryMultisig, TreasuryMultisigHostRef};
use cadence_treasury_multisig::types::ProposalStatus;
use odra::host::{Deployer, HostEnv};
use odra::prelude::Address;

struct Fixture {
    env: HostEnv,
    ms: TreasuryMultisigHostRef,
    alice: Address,
    bob: Address,
    carol: Address,
    outsider: Address,
}

/// Deploy a 2-of-3 multisig owned by accounts 0, 1, 2; account 3 is an outsider.
fn setup_2_of_3() -> Fixture {
    let env = odra_test::env();
    let alice = env.get_account(0);
    let bob = env.get_account(1);
    let carol = env.get_account(2);
    let outsider = env.get_account(3);
    env.set_caller(alice);
    let ms = TreasuryMultisig::deploy(
        &env,
        cadence_treasury_multisig::multisig::TreasuryMultisigInitArgs {
            owners: vec![alice, bob, carol],
            threshold: 2,
        },
    );
    Fixture { env, ms, alice, bob, carol, outsider }
}

fn action(tag: u8) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[0] = tag;
    h
}

#[test]
fn init_configures_owners_and_threshold() {
    let fx = setup_2_of_3();
    assert_eq!(fx.ms.threshold(), 2);
    assert_eq!(fx.ms.owner_count(), 3);
    assert_eq!(fx.ms.owners(), vec![fx.alice, fx.bob, fx.carol]);
    assert!(fx.ms.is_owner(fx.alice));
    assert!(fx.ms.is_owner(fx.carol));
    assert!(!fx.ms.is_owner(fx.outsider));
    assert_eq!(fx.ms.proposal_count(), 0);
}

/// Assert a deploy with the given owners/threshold reverts `InvalidConfiguration`.
///
/// `try_deploy`'s `Ok` variant is a `HostRef` that does not implement `Debug`, so
/// we `match` rather than `unwrap_err`.
fn assert_invalid_config(env: &HostEnv, owners: Vec<Address>, threshold: u32) {
    let result = TreasuryMultisig::try_deploy(
        env,
        cadence_treasury_multisig::multisig::TreasuryMultisigInitArgs { owners, threshold },
    );
    match result {
        Ok(_) => panic!("expected InvalidConfiguration, deploy succeeded"),
        Err(err) => assert_eq!(err, Error::InvalidConfiguration.into()),
    }
}

#[test]
fn empty_owner_set_is_rejected() {
    let env = odra_test::env();
    assert_invalid_config(&env, vec![], 1);
}

#[test]
fn zero_threshold_is_rejected() {
    let env = odra_test::env();
    let a = env.get_account(0);
    assert_invalid_config(&env, vec![a], 0);
}

#[test]
fn threshold_above_owner_count_is_rejected() {
    let env = odra_test::env();
    let a = env.get_account(0);
    let b = env.get_account(1);
    assert_invalid_config(&env, vec![a, b], 3);
}

#[test]
fn duplicate_owner_is_rejected() {
    let env = odra_test::env();
    let a = env.get_account(0);
    assert_invalid_config(&env, vec![a, a], 1);
}

#[test]
fn propose_creates_a_pending_proposal() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    assert_eq!(id, 0);
    assert_eq!(fx.ms.proposal_count(), 1);

    let p = fx.ms.get_proposal(0).expect("proposal exists");
    assert_eq!(p.id, 0);
    assert_eq!(p.proposer, fx.alice);
    assert_eq!(p.action_hash, action(1));
    assert_eq!(p.approvals, 0);
    assert_eq!(p.status, ProposalStatus::Pending);
}

#[test]
fn proposal_ids_are_dense_and_monotonic() {
    let mut fx = setup_2_of_3();
    assert_eq!(fx.ms.propose(action(1)), 0);
    assert_eq!(fx.ms.propose(action(2)), 1);
    assert_eq!(fx.ms.propose(action(3)), 2);
    assert_eq!(fx.ms.proposal_count(), 3);
}

#[test]
fn non_owner_cannot_propose() {
    let mut fx = setup_2_of_3();
    fx.env.set_caller(fx.outsider);
    let err = fx.ms.try_propose(action(1)).unwrap_err();
    assert_eq!(err, Error::NotOwner.into());
}

#[test]
fn approve_tallies_distinct_owners() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));

    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 1);
    assert!(fx.ms.has_approved(id, fx.alice));

    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 2);
    assert!(fx.ms.has_approved(id, fx.bob));
}

#[test]
fn double_approve_by_same_owner_is_rejected() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    let err = fx.ms.try_approve(id).unwrap_err();
    assert_eq!(err, Error::AlreadyApproved.into());
    // Tally unchanged.
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 1);
}

#[test]
fn non_owner_cannot_approve() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.outsider);
    let err = fx.ms.try_approve(id).unwrap_err();
    assert_eq!(err, Error::NotOwner.into());
}

#[test]
fn approve_unknown_proposal_reverts() {
    let mut fx = setup_2_of_3();
    fx.env.set_caller(fx.alice);
    let err = fx.ms.try_approve(99).unwrap_err();
    assert_eq!(err, Error::UnknownProposal.into());
}

// ----- quorum boundary: under / at / over -----

#[test]
fn execute_under_quorum_is_rejected() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id); // 1 of required 2

    let err = fx.ms.try_execute(id).unwrap_err();
    assert_eq!(err, Error::ThresholdNotMet.into());
    assert_eq!(fx.ms.get_proposal(id).unwrap().status, ProposalStatus::Pending);
}

#[test]
fn execute_at_quorum_succeeds() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(7));

    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id); // exactly threshold (2)

    fx.ms.execute(id);
    let p = fx.ms.get_proposal(id).unwrap();
    assert_eq!(p.status, ProposalStatus::Executed);
    assert_eq!(p.approvals, 2);
    assert_eq!(p.action_hash, action(7));
}

#[test]
fn execute_over_quorum_succeeds() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));

    // All three owners approve — over the threshold of 2.
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    fx.env.set_caller(fx.carol);
    fx.ms.approve(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 3);

    fx.ms.execute(id);
    assert_eq!(
        fx.ms.get_proposal(id).unwrap().status,
        ProposalStatus::Executed
    );
}

// ----- revoke -----

#[test]
fn revoke_decrements_tally_and_can_drop_below_quorum() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));

    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 2);

    // Bob changes his mind.
    fx.ms.revoke(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 1);
    assert!(!fx.ms.has_approved(id, fx.bob));

    // Now below quorum — execution must fail.
    fx.env.set_caller(fx.alice);
    let err = fx.ms.try_execute(id).unwrap_err();
    assert_eq!(err, Error::ThresholdNotMet.into());
}

#[test]
fn revoke_then_reapprove_reaches_quorum() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));

    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.ms.revoke(id);
    assert_eq!(fx.ms.get_proposal(id).unwrap().approvals, 0);

    // Re-approve is allowed after revoking.
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    fx.ms.execute(id);
    assert_eq!(
        fx.ms.get_proposal(id).unwrap().status,
        ProposalStatus::Executed
    );
}

#[test]
fn revoke_without_prior_approval_reverts() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.bob);
    let err = fx.ms.try_revoke(id).unwrap_err();
    assert_eq!(err, Error::NotApproved.into());
}

#[test]
fn non_owner_cannot_revoke() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.outsider);
    let err = fx.ms.try_revoke(id).unwrap_err();
    assert_eq!(err, Error::NotOwner.into());
}

// ----- replay / double-execute rejection -----

#[test]
fn double_execute_is_rejected() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    fx.ms.execute(id);

    // Second execution against the same id must revert.
    let err = fx.ms.try_execute(id).unwrap_err();
    assert_eq!(err, Error::AlreadyExecuted.into());
}

#[test]
fn approve_after_execution_is_rejected() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    fx.ms.execute(id);

    // Carol cannot approve an already-executed proposal.
    fx.env.set_caller(fx.carol);
    let err = fx.ms.try_approve(id).unwrap_err();
    assert_eq!(err, Error::AlreadyExecuted.into());
}

#[test]
fn revoke_after_execution_is_rejected() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);
    fx.ms.execute(id);

    let err = fx.ms.try_revoke(id).unwrap_err();
    assert_eq!(err, Error::AlreadyExecuted.into());
}

#[test]
fn execute_unknown_proposal_reverts() {
    let mut fx = setup_2_of_3();
    fx.env.set_caller(fx.alice);
    let err = fx.ms.try_execute(123).unwrap_err();
    assert_eq!(err, Error::UnknownProposal.into());
}

#[test]
fn non_owner_cannot_execute() {
    let mut fx = setup_2_of_3();
    let id = fx.ms.propose(action(1));
    fx.env.set_caller(fx.alice);
    fx.ms.approve(id);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id);

    fx.env.set_caller(fx.outsider);
    let err = fx.ms.try_execute(id).unwrap_err();
    assert_eq!(err, Error::NotOwner.into());
}

#[test]
fn proposals_are_independent() {
    let mut fx = setup_2_of_3();
    let id0 = fx.ms.propose(action(1));
    let id1 = fx.ms.propose(action(2));

    fx.env.set_caller(fx.alice);
    fx.ms.approve(id0);
    fx.env.set_caller(fx.bob);
    fx.ms.approve(id0);
    fx.ms.execute(id0);

    // id1 is untouched and still pending with no approvals.
    let p1 = fx.ms.get_proposal(id1).unwrap();
    assert_eq!(p1.status, ProposalStatus::Pending);
    assert_eq!(p1.approvals, 0);
}
