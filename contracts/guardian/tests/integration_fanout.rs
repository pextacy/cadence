//! Wave 3 integration: the desk-wide [`Guardian`] fans a single `global_pause`
//! out to several REAL `ExecutionVault`s registered in a real `VaultRegistry`,
//! proving one call halts the whole desk — and that the sweep tolerates a vault an
//! earlier actor already paused (the idempotent `VaultControl` contract).

use cadence_guardian::guardian::{Guardian, GuardianHostRef, GuardianInitArgs};
use cadence_vault::vault::{
    ExecutionVault, ExecutionVaultHostRef, ExecutionVaultInitArgs, MANDATE_DOMAIN_TAG,
};
use cadence_vault::vault::Status;
use cadence_vault_registry::registry::{VaultRegistry, VaultRegistryHostRef};
use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::U512;
use odra::host::{Deployer, HostEnv, HostRef, NoArgs};
use odra::prelude::{Address, Addressable};

const TOTAL_SELL: u64 = 1_000_000;
const END_TIME_MS: u64 = 1_000_000;
const SLIPPAGE_BPS: u32 = 100;

/// Reconstruct the canonical mandate preimage the vault signs over at `init`,
/// byte-for-byte identical to `ExecutionVault::mandate_message`.
#[allow(clippy::too_many_arguments)]
fn mandate_preimage(
    agent: Address,
    treasury: Address,
    venues: &[String],
    venue_addresses: &[Address],
    nonce: &Bytes,
) -> Bytes {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(MANDATE_DOMAIN_TAG);
    buf.extend_from_slice(&agent.to_bytes().unwrap());
    buf.extend_from_slice(&treasury.to_bytes().unwrap());
    buf.extend_from_slice(&"CSPR".to_string().to_bytes().unwrap());
    buf.extend_from_slice(&"USDC".to_string().to_bytes().unwrap());
    buf.extend_from_slice(&U512::from(TOTAL_SELL).to_bytes().unwrap());
    buf.extend_from_slice(&END_TIME_MS.to_bytes().unwrap());
    buf.extend_from_slice(&SLIPPAGE_BPS.to_bytes().unwrap());
    buf.extend_from_slice(&U512::zero().to_bytes().unwrap());
    buf.extend_from_slice(&U512::zero().to_bytes().unwrap());
    buf.extend_from_slice(&venues.to_vec().to_bytes().unwrap());
    buf.extend_from_slice(&venue_addresses.to_vec().to_bytes().unwrap());
    buf.extend_from_slice(&nonce.to_bytes().unwrap());
    Bytes::from(buf)
}

/// Deploy, fund and activate one real vault owned by `treasury`.
fn deploy_funded_vault(
    env: &HostEnv,
    treasury: Address,
    agent: Address,
    venue_addr: Address,
    nonce_byte: u8,
) -> ExecutionVaultHostRef {
    let venues = vec!["cspr.trade".to_string()];
    let nonce = Bytes::from(vec![nonce_byte; 32]);
    env.set_caller(treasury);
    let preimage = mandate_preimage(agent, treasury, &venues, &[venue_addr], &nonce);
    let casper_signature = env.sign_message(&preimage, &treasury);
    let treasury_pk = env.public_key(&treasury);
    let vault = ExecutionVault::deploy(
        env,
        ExecutionVaultInitArgs {
            agent,
            mandate_digest: Bytes::from(vec![7u8; 32]),
            signature: Bytes::from(vec![1u8; 65]),
            treasury_public_key: treasury_pk,
            casper_signature,
            mandate_nonce: nonce,
            sell_asset: "CSPR".to_string(),
            buy_asset: "USDC".to_string(),
            total_sell: U512::from(TOTAL_SELL),
            end_time_ms: END_TIME_MS,
            max_slippage_bps: SLIPPAGE_BPS,
            price_floor: U512::zero(),
            price_ceiling: U512::zero(),
            venues,
            venue_addresses: vec![venue_addr],
        },
    );
    env.set_caller(treasury);
    vault.with_tokens(U512::from(TOTAL_SELL)).fund();
    vault
}

struct Desk {
    env: HostEnv,
    guardian: GuardianHostRef,
    v1: ExecutionVaultHostRef,
    v2: ExecutionVaultHostRef,
}

/// Stand up a registry + two registered, guardian-wired vaults + a guardian.
fn setup_desk() -> Desk {
    let env = odra_test::env();
    let treasury = env.get_account(0);
    let agent = env.get_account(1);
    let venue = env.get_account(2);

    env.set_caller(treasury);
    let mut registry: VaultRegistryHostRef = VaultRegistry::deploy(&env, NoArgs);

    let mut v1 = deploy_funded_vault(&env, treasury, agent, venue, 5);
    let mut v2 = deploy_funded_vault(&env, treasury, agent, venue, 6);

    env.set_caller(treasury);
    registry.register(v1.address(), treasury, [0u8; 32]);
    registry.register(v2.address(), treasury, [1u8; 32]);

    env.set_caller(treasury);
    let guardian: GuardianHostRef = Guardian::deploy(
        &env,
        GuardianInitArgs { registry: registry.address() },
    );

    // Each vault wires the guardian CONTRACT as a GUARDIAN so it may pause it.
    env.set_caller(treasury);
    v1.set_guardian(guardian.address());
    env.set_caller(treasury);
    v2.set_guardian(guardian.address());

    Desk { env, guardian, v1, v2 }
}

#[test]
fn one_global_pause_halts_every_registered_vault() {
    let mut desk = setup_desk();
    let treasury = desk.env.get_account(0);

    assert_eq!(desk.v1.get_status(), Status::Active);
    assert_eq!(desk.v2.get_status(), Status::Active);

    desk.env.set_caller(treasury);
    let affected = desk.guardian.global_pause(0, 16);

    assert_eq!(affected, 2, "both active vaults were paused by one call");
    assert_eq!(desk.v1.get_status(), Status::Paused);
    assert_eq!(desk.v2.get_status(), Status::Paused);
    assert!(desk.guardian.is_paused());
}

#[test]
fn fan_out_tolerates_an_already_paused_vault() {
    let mut desk = setup_desk();
    let treasury = desk.env.get_account(0);
    let agent = desk.env.get_account(1);

    // The agent locally pauses v1 before the desk-wide sweep.
    desk.env.set_caller(agent);
    desk.v1.pause();
    assert_eq!(desk.v1.get_status(), Status::Paused);

    // The sweep must NOT abort on the already-paused vault (idempotent pause).
    desk.env.set_caller(treasury);
    let affected = desk.guardian.global_pause(0, 16);
    assert_eq!(affected, 2);
    assert_eq!(desk.v1.get_status(), Status::Paused);
    assert_eq!(desk.v2.get_status(), Status::Paused);
}

#[test]
fn non_guardian_cannot_trigger_a_global_pause() {
    let mut desk = setup_desk();
    let agent = desk.env.get_account(1);
    desk.env.set_caller(agent);
    assert!(desk.guardian.try_global_pause(0, 16).is_err());
}
