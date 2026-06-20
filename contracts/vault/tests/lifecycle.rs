//! Lifecycle and admin behaviour: funding, settlement, the one-shot constructor,
//! and the `emergency_withdraw` circuit-breaker drain.

mod common;

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::host::HostRef;

use cadence_vault::vault::{Error, Status};
use common::*;

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
        )
        .unwrap_err();
    assert_eq!(err, Error::NotAgent.into());
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
    // The mandate's limits are immutable for the life of the vault. Odra enforces
    // this at the framework level: `init` is a constructor and cannot be
    // re-invoked after install, so a second attempt must error.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fx.env.set_caller(fx.treasury);
    let pk = fx.env.public_key(&fx.treasury);
    let result = fx.contract.try_init(
        fx.agent,
        digest32(),
        Bytes::from(vec![1u8; 65]),
        pk,
        Bytes::from(vec![1u8; 65]),
        nonce32(),
        "CSPR".to_string(),
        "USDC".to_string(),
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        venues(),
        vec![fx.venue_addr],
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

// ---------------------------------------------------------------------------
// emergency_withdraw — treasury-only, requires Paused
// ---------------------------------------------------------------------------

#[test]
fn emergency_withdraw_drains_to_treasury_when_paused() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    ok_slice(&mut fx); // releases 100_000, leaves 900_000 in the vault

    // Treasury pauses, then drains.
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    assert_eq!(fx.contract.get_status(), Status::Paused);

    let before = fx.env.balance_of(&fx.treasury);
    fx.env.set_caller(fx.treasury);
    fx.contract.emergency_withdraw();
    assert_eq!(fx.contract.get_status(), Status::Halted);
    let after = fx.env.balance_of(&fx.treasury);
    assert!(after > before, "remaining balance must return to treasury");
}

#[test]
fn treasury_can_wire_a_guardian_that_pauses() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    let guardian = fx.env.get_account(3);

    // An unwired account cannot pause (not agent/treasury/guardian).
    assert!(!fx.contract.is_guardian(guardian));
    fx.env.set_caller(guardian);
    assert_eq!(
        fx.contract.try_pause().unwrap_err(),
        Error::NotAgent.into(),
        "a non-role account must not be able to pause"
    );

    // Treasury wires the guardian (e.g. the desk-wide Guardian contract).
    fx.env.set_caller(fx.treasury);
    fx.contract.set_guardian(guardian);
    assert!(fx.contract.is_guardian(guardian));

    // The guardian can now pause the vault.
    fx.env.set_caller(guardian);
    fx.contract.pause();
    assert_eq!(fx.contract.get_status(), Status::Paused);

    // Only the treasury may wire a guardian.
    fx.env.set_caller(fx.agent);
    assert_eq!(
        fx.contract.try_set_guardian(fx.agent).unwrap_err(),
        Error::NotTreasury.into(),
        "only the treasury may set a guardian"
    );
}

#[test]
fn emergency_withdraw_rejects_non_treasury() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    // Agent attempts the drain — must be rejected.
    fx.env.set_caller(fx.agent);
    let err = fx.contract.try_emergency_withdraw().unwrap_err();
    assert_eq!(err, Error::NotTreasury.into());
}

#[test]
fn emergency_withdraw_requires_paused() {
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx); // Active, not Paused
    fx.env.set_caller(fx.treasury);
    let err = fx.contract.try_emergency_withdraw().unwrap_err();
    assert_eq!(err, Error::NotPaused.into());
}

#[test]
fn settle_blocked_after_emergency_withdraw() {
    // Once halted, settle must not move funds again (terminal state).
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    fx.env.set_caller(fx.treasury);
    fx.contract.emergency_withdraw();
    assert_eq!(fx.contract.get_status(), Status::Halted);

    fx.env.advance_block_time(END_TIME_MS + 1);
    let err = fx.contract.try_settle().unwrap_err();
    assert_eq!(err, Error::CannotSettleYet.into());
}

#[test]
fn execute_slice_blocked_while_halted() {
    // After an emergency drain the agent cannot resume execution: status is
    // Halted (not Active), so execute_slice reverts NotActive.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    fx.env.set_caller(fx.treasury);
    fx.contract.emergency_withdraw();

    fx.env.set_caller(fx.agent);
    let err = fx
        .contract
        .try_execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
        )
        .unwrap_err();
    assert_eq!(err, Error::NotActive.into());
}

#[test]
fn pause_is_idempotent_when_already_paused() {
    // The desk-wide Guardian fan-out relies on pause being a no-op (not a revert)
    // when a vault is already Paused — e.g. the vault's own agent tripped the
    // breaker before the sweep reached it. A second pause must succeed and leave
    // the status unchanged.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    assert_eq!(fx.contract.get_status(), Status::Paused);

    // Re-pausing must NOT revert, and the status stays Paused.
    fx.env.set_caller(fx.treasury);
    fx.contract.try_pause().expect("re-pausing is a no-op");
    assert_eq!(fx.contract.get_status(), Status::Paused);
}

#[test]
fn resume_is_idempotent_when_already_active() {
    // Mirror of the pause case: a desk-wide global_resume must tolerate a vault
    // that is already Active without aborting the sweep.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    assert_eq!(fx.contract.get_status(), Status::Active);

    // Resuming an Active vault is a no-op, not a revert.
    fx.env.set_caller(fx.treasury);
    fx.contract
        .try_resume()
        .expect("resuming Active is a no-op");
    assert_eq!(fx.contract.get_status(), Status::Active);
}

#[test]
fn pause_still_reverts_from_a_non_pausable_state() {
    // Idempotency only swallows the already-in-target-state case. Pausing a
    // terminal (Halted) vault must still revert NotActive — the no-op path must
    // not silently accept a meaningless pause on a dead vault.
    let mut fx = deploy_with(U512::zero(), U512::zero());
    fund(&mut fx);
    fx.env.set_caller(fx.treasury);
    fx.contract.pause();
    fx.env.set_caller(fx.treasury);
    fx.contract.emergency_withdraw();
    assert_eq!(fx.contract.get_status(), Status::Halted);

    fx.env.set_caller(fx.treasury);
    let err = fx.contract.try_pause().unwrap_err();
    assert_eq!(err, Error::NotActive.into());
}
