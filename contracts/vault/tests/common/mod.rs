//! Shared test fixtures for the vault integration tests.
//!
//! These helpers are the verbatim deploy / fund / slice scaffolding that used to
//! live in the inline `#[cfg(test)] mod tests` of `vault.rs`, lifted unchanged so
//! the behavioural and adversarial suites split across `tests/*.rs` share one
//! source of truth.

#![allow(dead_code)]

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef};
use odra::prelude::{Address, OdraError};

use cadence_vault::vault::{
    ExecutionVault, ExecutionVaultHostRef, ExecutionVaultInitArgs, MANDATE_DOMAIN_TAG,
};

pub const TOTAL_SELL: u64 = 1_000_000;
pub const END_TIME_MS: u64 = 1_000_000;
pub const SLIPPAGE_BPS: u32 = 100; // 1%

pub struct Fixture {
    pub env: HostEnv,
    pub contract: ExecutionVaultHostRef,
    pub treasury: Address,
    pub agent: Address,
    pub venue_addr: Address,
}

pub fn digest32() -> Bytes {
    Bytes::from(vec![7u8; 32])
}

pub fn nonce32() -> Bytes {
    Bytes::from(vec![5u8; 32])
}

pub fn venues() -> Vec<String> {
    vec!["cspr.trade".to_string()]
}

/// Reconstruct the exact canonical preimage the contract signs over, off-chain.
/// Byte-for-byte identical to `ExecutionVault::mandate_message`.
#[allow(clippy::too_many_arguments)]
pub fn mandate_message_offchain(
    agent: Address,
    treasury: Address,
    sell_asset: &str,
    buy_asset: &str,
    total_sell: U512,
    end_time_ms: u64,
    max_slippage_bps: u32,
    price_floor: U512,
    price_ceiling: U512,
    venues: &[String],
    venue_addresses: &[Address],
    nonce: &Bytes,
) -> Bytes {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(MANDATE_DOMAIN_TAG);
    buf.extend_from_slice(&agent.to_bytes().unwrap());
    buf.extend_from_slice(&treasury.to_bytes().unwrap());
    buf.extend_from_slice(&sell_asset.to_string().to_bytes().unwrap());
    buf.extend_from_slice(&buy_asset.to_string().to_bytes().unwrap());
    buf.extend_from_slice(&total_sell.to_bytes().unwrap());
    buf.extend_from_slice(&end_time_ms.to_bytes().unwrap());
    buf.extend_from_slice(&max_slippage_bps.to_bytes().unwrap());
    buf.extend_from_slice(&price_floor.to_bytes().unwrap());
    buf.extend_from_slice(&price_ceiling.to_bytes().unwrap());
    buf.extend_from_slice(&venues.to_vec().to_bytes().unwrap());
    buf.extend_from_slice(&venue_addresses.to_vec().to_bytes().unwrap());
    buf.extend_from_slice(&nonce.to_bytes().unwrap());
    Bytes::from(buf)
}

/// All-purpose deploy helper. The preimage is address-independent (bound by the
/// nonce, not `self_address`), so the treasury can sign it before the vault
/// exists — exactly the production flow. Parameters let the adversarial tests
/// deliberately mis-sign or tamper the installed limits.
pub struct DeployArgs {
    pub price_floor: U512,
    pub price_ceiling: U512,
    /// Account whose key signs the canonical preimage.
    pub signer: Address,
    /// Account whose public key is supplied to `init`.
    pub supplied_pk_account: Address,
    /// total_sell value baked into the SIGNED preimage.
    pub signed_total: U512,
    /// total_sell value actually INSTALLED via init (differs to simulate tamper).
    pub install_total: U512,
    /// If set, an explicit raw signature to use instead of signing the preimage
    /// (for the forged-bytes case).
    pub override_signature: Option<Bytes>,
}

pub fn try_deploy(env: &HostEnv, args: DeployArgs) -> Result<ExecutionVaultHostRef, OdraError> {
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    let venue_addr = env.get_account(2);
    env.set_caller(treasury);

    let preimage = mandate_message_offchain(
        agent,
        treasury,
        "CSPR",
        "USDC",
        args.signed_total,
        END_TIME_MS,
        SLIPPAGE_BPS,
        args.price_floor,
        args.price_ceiling,
        &venues(),
        &[venue_addr],
        &nonce32(),
    );
    let casper_signature = args
        .override_signature
        .unwrap_or_else(|| env.sign_message(&preimage, &args.signer));
    let supplied_pk = env.public_key(&args.supplied_pk_account);

    ExecutionVault::try_deploy(
        env,
        ExecutionVaultInitArgs {
            agent,
            mandate_digest: digest32(),
            signature: Bytes::from(vec![1u8; 65]),
            treasury_public_key: supplied_pk,
            casper_signature,
            mandate_nonce: nonce32(),
            sell_asset: "CSPR".to_string(),
            buy_asset: "USDC".to_string(),
            total_sell: args.install_total,
            end_time_ms: END_TIME_MS,
            max_slippage_bps: SLIPPAGE_BPS,
            price_floor: args.price_floor,
            price_ceiling: args.price_ceiling,
            venues: venues(),
            venue_addresses: vec![venue_addr],
        },
    )
}

pub fn deploy_with(price_floor: U512, price_ceiling: U512) -> Fixture {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    let venue_addr = env.get_account(2);
    let contract = try_deploy(
        &env,
        DeployArgs {
            price_floor,
            price_ceiling,
            signer: treasury,
            supplied_pk_account: treasury,
            signed_total: U512::from(TOTAL_SELL),
            install_total: U512::from(TOTAL_SELL),
            override_signature: None,
        },
    )
    .expect("happy-path deploy must verify");
    Fixture {
        env,
        contract,
        treasury,
        agent,
        venue_addr,
    }
}

pub fn fund(fx: &mut Fixture) {
    fx.env.set_caller(fx.treasury);
    fx.contract.with_tokens(U512::from(TOTAL_SELL)).fund();
}

/// A slice priced at 2.0 with exactly 1% slippage — passes all guardrails.
pub fn ok_slice(fx: &mut Fixture) -> u32 {
    fx.env.set_caller(fx.agent);
    fx.contract.execute_slice(
        U512::from(100_000u64),
        U512::from(200_000u64),
        U512::from(198_000u64),
        "cspr.trade".to_string(),
    )
}
