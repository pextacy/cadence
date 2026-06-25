//! Wave 2b integration: an off-chain (escrow) venue settles through the
//! `SettlementAdapter`, and the vault credits the fill from the operator-attested
//! settlement — never from an agent-supplied amount.
//!
//! Proves the production-honest path for cspr.trade (an off-chain MCP DEX with no
//! atomic on-chain router):
//!   1. treasury opts the venue into adapter routing;
//!   2. the agent's `execute_slice` escrows the slice into the adapter (non-atomic),
//!      so `bought_so_far` stays 0 and the slice is linked to escrow id 0;
//!   3. the unproven `record_fill` is REJECTED for the escrow slice;
//!   4. `record_escrow_fill` reverts until a settlement exists;
//!   5. the settlement operator signs the canonical preimage and proves the realised
//!      amount via `record_settlement` on the adapter;
//!   6. `record_escrow_fill` then credits the PROVEN amount to `bought_so_far`.

mod common;

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::Address;

use cadence_dex_adapter::settlement::{
    settlement_message, SettlementAdapter, SettlementAdapterHostRef, SettlementAdapterInitArgs,
};
use cadence_vault::vault::{Error, ExecutionVault, ExecutionVaultHostRef, ExecutionVaultInitArgs};

use common::*;

const VENUE: &str = "cspr.trade";

/// Deploy a real `SettlementAdapter` (operator = account 2) plus a vault whose
/// single venue address IS that adapter; fund the vault and opt the venue into
/// adapter routing. Returns the live refs and the key accounts.
fn setup() -> (
    HostEnv,
    ExecutionVaultHostRef,
    SettlementAdapterHostRef,
    Address, // treasury (escrow recipient)
    Address, // operator
) {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    let operator = env.get_account(2);
    env.set_caller(treasury);

    // 1. The escrow + attestation adapter for the off-chain venue.
    let adapter = SettlementAdapter::deploy(
        &env,
        SettlementAdapterInitArgs {
            venue_id: VENUE.to_string(),
            operator,
        },
    );
    let adapter_addr = adapter.contract_address();

    // 2. Sign a mandate whose venue address IS the adapter, and deploy the vault.
    let preimage = mandate_message_offchain(
        agent,
        treasury,
        "CSPR",
        "USDC",
        U512::from(TOTAL_SELL),
        END_TIME_MS,
        SLIPPAGE_BPS,
        U512::zero(),
        U512::zero(),
        &venues(),
        &[adapter_addr],
        &nonce32(),
    );
    let casper_signature = env.sign_message(&preimage, &treasury);
    let mut vault = ExecutionVault::deploy(
        &env,
        ExecutionVaultInitArgs {
            agent,
            mandate_digest: digest32(),
            signature: Bytes::from(vec![1u8; 65]),
            treasury_public_key: env.public_key(&treasury),
            casper_signature,
            mandate_nonce: nonce32(),
            sell_asset: "CSPR".to_string(),
            buy_asset: "USDC".to_string(),
            total_sell: U512::from(TOTAL_SELL),
            end_time_ms: END_TIME_MS,
            max_slippage_bps: SLIPPAGE_BPS,
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            venues: venues(),
            venue_addresses: vec![adapter_addr],
        },
    );

    // 3. Fund and opt the venue into cross-contract adapter routing.
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault.set_venue_adapter(VENUE.to_string(), true);

    (env, vault, adapter, treasury, operator)
}

/// Agent executes one slice; the vault escrows it into the adapter (non-atomic).
/// Returns the slice id (== escrow id == 0 for the first slice).
fn escrow_one_slice(env: &HostEnv, vault: &mut ExecutionVaultHostRef, agent: Address) -> u32 {
    env.set_caller(agent);
    vault.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        VENUE.to_string(),
    )
}

/// Operator signs the canonical settlement preimage for an escrow and proves the
/// realised `bought` amount on the adapter.
fn settle(
    env: &HostEnv,
    adapter: &mut SettlementAdapterHostRef,
    recipient: Address,
    operator: Address,
    escrow_id: u64,
    bought: U512,
) {
    let nonce = Bytes::from(vec![7u8; 32]);
    let settlement_ref = Bytes::from(b"cspr-trade-deploy".to_vec());
    let addr = adapter.contract_address();
    let msg = settlement_message(&addr, escrow_id, bought, &settlement_ref, &nonce, &recipient)
        .expect("preimage serializes");
    let sig = env.sign_message(&msg, &operator);
    let pk = env.public_key(&operator);
    // Anyone may submit; only the operator signature authorises it.
    env.set_caller(recipient);
    adapter.record_settlement(escrow_id, bought, settlement_ref, nonce, pk, sig);
}

#[test]
fn escrow_slice_credits_only_the_operator_attested_amount() {
    let (env, mut vault, mut adapter, treasury, operator) = setup();
    let agent = env.get_account(1);

    // Escrow the slice: non-atomic, so nothing is bought on-chain yet.
    let slice_id = escrow_one_slice(&env, &mut vault, agent);
    assert_eq!(slice_id, 0);
    assert_eq!(vault.get_sold_so_far(), U512::from(100_000u64));
    assert_eq!(
        vault.get_bought_so_far(),
        U512::zero(),
        "escrow venue buys nothing until the attested settlement"
    );

    // The operator proves a realised 199_000 on the adapter, then the agent credits
    // it via record_escrow_fill — the vault reads the PROVEN amount.
    settle(
        &env,
        &mut adapter,
        treasury,
        operator,
        0,
        U512::from(199_000u64),
    );
    env.set_caller(agent);
    vault.record_escrow_fill(slice_id);
    assert_eq!(
        vault.get_bought_so_far(),
        U512::from(199_000u64),
        "vault must credit the operator-attested amount"
    );
}

#[test]
fn record_fill_is_rejected_for_an_escrow_slice() {
    // The unproven agent-supplied path must not credit an off-chain venue's fill.
    let (env, mut vault, _adapter, _treasury, _operator) = setup();
    let agent = env.get_account(1);
    let slice_id = escrow_one_slice(&env, &mut vault, agent);

    env.set_caller(agent);
    let err = vault
        .try_record_fill(slice_id, U512::from(199_000u64), "forged".to_string())
        .unwrap_err();
    assert_eq!(err, Error::UseEscrowFill.into());
}

#[test]
fn record_escrow_fill_reverts_before_settlement() {
    // The agent cannot advance the fill ahead of the operator's on-chain proof.
    let (env, mut vault, _adapter, _treasury, _operator) = setup();
    let agent = env.get_account(1);
    let slice_id = escrow_one_slice(&env, &mut vault, agent);

    env.set_caller(agent);
    let err = vault.try_record_escrow_fill(slice_id).unwrap_err();
    assert_eq!(err, Error::EscrowNotSettled.into());
}

#[test]
fn escrow_fill_cannot_be_double_credited() {
    let (env, mut vault, mut adapter, treasury, operator) = setup();
    let agent = env.get_account(1);
    let slice_id = escrow_one_slice(&env, &mut vault, agent);
    settle(
        &env,
        &mut adapter,
        treasury,
        operator,
        0,
        U512::from(199_000u64),
    );

    env.set_caller(agent);
    vault.record_escrow_fill(slice_id);
    // A second credit for the same slice must revert rather than double-count.
    env.set_caller(agent);
    let err = vault.try_record_escrow_fill(slice_id).unwrap_err();
    assert_eq!(err, Error::SliceAlreadyFilled.into());
}

#[test]
fn record_escrow_fill_rejects_a_direct_transfer_slice() {
    // A slice that did NOT route through an escrow adapter has no settlement to read.
    let (env, mut vault, _adapter, _treasury, _operator) = setup();
    let agent = env.get_account(1);

    // Turn adapter routing OFF so the next slice is a direct transfer.
    let treasury = env.get_account(0);
    env.set_caller(treasury);
    vault.set_venue_adapter(VENUE.to_string(), false);

    let slice_id = escrow_one_slice(&env, &mut vault, agent);
    env.set_caller(agent);
    let err = vault.try_record_escrow_fill(slice_id).unwrap_err();
    assert_eq!(err, Error::NotEscrowSlice.into());
}
