//! `SettlementAdapter` — the reference adapter for off-chain MCP venues (cspr.trade).
//!
//! The design findings (and `agent/src/clients/csprTrade.ts`) establish that
//! cspr.trade is an OFF-CHAIN MCP DEX reached over HTTP at `https://mcp.cspr.trade`;
//! it exposes no on-chain router a WASM contract can call atomically. Therefore an
//! atomic on-chain swap against it is infeasible. The production-honest shape is a
//! two-phase escrow + signed-attestation flow:
//!
//! 1. **Escrow (`swap`)** — the vault routes the slice here; the adapter takes
//!    custody of the native sell asset attached to the call, books a per-slice
//!    escrow record, emits a [`SwapIntent`], and returns `atomic = false`. No buy
//!    asset has arrived yet, so `bought_amount` is `0`.
//! 2. **Attested settlement (`record_settlement`)** — after the agent executes the
//!    off-chain swap, the settlement operator signs a canonical preimage over the
//!    realised `bought_amount` and `settlement_ref` and submits it. The adapter
//!    verifies the signature on-chain with `env().verify_signature` against the
//!    registered operator key (the exact x402-token pattern), enforces `min_out`,
//!    replay-protects on `(operator, nonce)`, releases the escrowed sell asset to
//!    the configured sink, and emits [`SettlementRecorded`] — the proof the vault
//!    trusts in place of an unverified `swap_deploy_hash` string.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use crate::adapter::SwapReceipt;

/// Emitted when the vault escrows a slice for off-chain settlement. The agent and
/// settlement operator watch for this to know a swap is owed and which slice/escrow
/// it corresponds to.
#[odra::event]
pub struct SwapIntent {
    pub escrow_id: u64,
    pub vault: Address,
    pub sell_asset: String,
    pub buy_asset: String,
    pub sell_amount: U512,
    pub min_out: U512,
    pub recipient: Address,
}

/// Emitted after a settlement operator's attestation verifies on-chain, proving the
/// realised buy-asset amount for an escrowed slice.
#[odra::event]
pub struct SettlementRecorded {
    pub escrow_id: u64,
    pub bought_amount: U512,
    pub settlement_ref: Bytes,
    pub nonce: Bytes,
}

#[odra::odra_error]
pub enum Error {
    /// `swap` was called with a zero sell amount.
    ZeroSellAmount = 1,
    /// The native value attached to `swap` did not equal `sell_amount`.
    EscrowAmountMismatch = 2,
    /// `record_settlement` referenced an unknown escrow id.
    UnknownEscrow = 3,
    /// The escrow has already been settled — one settlement per escrow.
    EscrowAlreadySettled = 4,
    /// Realised `bought_amount` is below the escrow's committed `min_out`.
    SlippageTooHigh = 5,
    /// The supplied public key does not hash to the registered operator account.
    NotAuthorizedSigner = 6,
    /// This `(operator, nonce)` attestation has already been used.
    AttestationAlreadyUsed = 7,
    /// The signature does not verify against the settlement preimage.
    BadSignature = 8,
    /// Failed to serialize the settlement preimage.
    SerializationError = 9,
    /// Caller is not the configured operator (administrative entrypoints).
    NotOperator = 10,
}

/// One escrowed slice awaiting off-chain settlement.
#[odra::odra_type]
pub struct Escrow {
    pub vault: Address,
    pub sell_amount: U512,
    pub min_out: U512,
    pub recipient: Address,
    pub settled: bool,
}

/// Escrow + attested-settlement adapter for off-chain venues.
#[odra::module(events = [SwapIntent, SettlementRecorded], errors = Error)]
pub struct SettlementAdapter {
    /// Stable venue id this adapter settles for (e.g. `"cspr.trade"`).
    venue_id: Var<String>,
    /// Settlement operator account; its key signs realised-fill attestations.
    operator: Var<Address>,
    /// Monotonic escrow id counter.
    next_escrow_id: Var<u64>,
    /// Per-escrow custody + terms record.
    escrows: Mapping<u64, Escrow>,
    /// Spent attestation nonces, keyed by `(operator, nonce)` for replay protection.
    used_attestations: Mapping<(Address, Bytes), bool>,
}

#[odra::module]
impl SettlementAdapter {
    /// Initialise with the venue id and the settlement operator account whose key
    /// will sign realised-fill attestations.
    pub fn init(&mut self, venue_id: String, operator: Address) {
        self.venue_id.set(venue_id);
        self.operator.set(operator);
        self.next_escrow_id.set(0);
    }

    // ----- VenueAdapter entrypoints (called cross-contract by the vault) -----

    /// Escrow `sell_amount` of the native sell asset for off-chain settlement.
    ///
    /// The vault attaches `sell_amount` native tokens to this call. We take custody,
    /// book an escrow, emit the intent, and return `atomic = false` so the vault
    /// knows the fill will be proven later via [`Self::record_settlement`].
    #[odra(payable)]
    pub fn swap(
        &mut self,
        sell_asset: String,
        buy_asset: String,
        sell_amount: U512,
        min_out: U512,
        recipient: Address,
    ) -> SwapReceipt {
        if sell_amount.is_zero() {
            self.env().revert(Error::ZeroSellAmount);
        }
        // The vault must attach exactly the sell amount — the adapter custodies it.
        if self.env().attached_value() != sell_amount {
            self.env().revert(Error::EscrowAmountMismatch);
        }
        let vault = self.env().caller();
        let escrow_id = self.next_escrow_id.get_or_default();
        self.escrows.set(
            &escrow_id,
            Escrow {
                vault,
                sell_amount,
                min_out,
                recipient,
                settled: false,
            },
        );
        self.next_escrow_id.set(escrow_id + 1);

        self.env().emit_event(SwapIntent {
            escrow_id,
            vault,
            sell_asset,
            buy_asset,
            sell_amount,
            min_out,
            recipient,
        });

        // No buy asset on-chain yet: report a non-atomic receipt. The settlement ref
        // is the escrow id, so the off-chain settlement can be linked back.
        SwapReceipt {
            bought_amount: U512::zero(),
            settlement_ref: Bytes::from(escrow_id.to_le_bytes().to_vec()),
            atomic: false,
        }
    }

    /// Stable venue identifier.
    pub fn venue_id(&self) -> String {
        self.venue_id.get_or_default()
    }

    // ----- attested settlement -----

    /// Prove the realised fill for an escrowed slice with an operator signature.
    ///
    /// `operator_pk` must hash to the registered operator; `signature` must verify
    /// against the canonical preimage (see [`Self::settlement_message`]). Enforces
    /// `min_out`, replay-protects on `(operator, nonce)`, releases the escrowed sell
    /// asset to the recipient, and emits [`SettlementRecorded`]. Any account may
    /// submit it — only a valid operator signature authorises it.
    #[allow(clippy::too_many_arguments)]
    pub fn record_settlement(
        &mut self,
        escrow_id: u64,
        bought_amount: U512,
        settlement_ref: Bytes,
        nonce: Bytes,
        operator_pk: PublicKey,
        signature: Bytes,
    ) {
        // 1. supplied key must be the registered operator's key.
        let operator = self.operator.get_or_revert_with(Error::NotOperator);
        if Address::from(operator_pk.clone()) != operator {
            self.env().revert(Error::NotAuthorizedSigner);
        }
        // 2. escrow must exist and be unsettled.
        let mut escrow = match self.escrows.get(&escrow_id) {
            Some(e) => e,
            None => self.env().revert(Error::UnknownEscrow),
        };
        if escrow.settled {
            self.env().revert(Error::EscrowAlreadySettled);
        }
        // 3. realised output must clear the committed floor.
        if bought_amount < escrow.min_out {
            self.env().revert(Error::SlippageTooHigh);
        }
        // 4. attestation nonce must be unused.
        let nonce_key = (operator, nonce.clone());
        if self.used_attestations.get_or_default(&nonce_key) {
            self.env().revert(Error::AttestationAlreadyUsed);
        }
        // 5. the signature must verify against the canonical preimage.
        let message = self.settlement_message(
            escrow_id,
            bought_amount,
            &settlement_ref,
            &nonce,
            escrow.recipient,
        );
        if !self
            .env()
            .verify_signature(&message, &signature, &operator_pk)
        {
            self.env().revert(Error::BadSignature);
        }
        // 6. effects-before-interactions: mark settled and spend the nonce, then
        //    release the escrowed sell asset to the recipient (the venue/agent leg).
        escrow.settled = true;
        let recipient = escrow.recipient;
        let sell_amount = escrow.sell_amount;
        self.escrows.set(&escrow_id, escrow);
        self.used_attestations.set(&nonce_key, true);
        self.env().transfer_tokens(&recipient, &sell_amount);

        self.env().emit_event(SettlementRecorded {
            escrow_id,
            bought_amount,
            settlement_ref,
            nonce,
        });
    }

    // ----- views -----

    pub fn get_operator(&self) -> Address {
        self.operator.get_or_revert_with(Error::NotOperator)
    }

    pub fn get_escrow(&self, escrow_id: u64) -> Option<Escrow> {
        self.escrows.get(&escrow_id)
    }

    pub fn attestation_used(&self, operator: Address, nonce: Bytes) -> bool {
        self.used_attestations.get_or_default(&(operator, nonce))
    }

    // ----- internal helpers (private — never exposed as entrypoints) -----

    /// Canonical settlement preimage the operator signs and the contract verifies.
    /// Prefixed with the adapter's own address to bind the attestation to *this*
    /// adapter (cross-contract replay protection), then every settlement field in
    /// Casper `ToBytes` encoding — the same discipline as `token.rs:221-247`.
    fn settlement_message(
        &self,
        escrow_id: u64,
        bought_amount: U512,
        settlement_ref: &Bytes,
        nonce: &Bytes,
        recipient: Address,
    ) -> Bytes {
        let parts: [Result<Vec<u8>, _>; 6] = [
            self.env().self_address().to_bytes(),
            escrow_id.to_bytes(),
            bought_amount.to_bytes(),
            settlement_ref.to_bytes(),
            nonce.to_bytes(),
            recipient.to_bytes(),
        ];
        let mut buf: Vec<u8> = Vec::new();
        for part in parts {
            match part {
                Ok(bytes) => buf.extend_from_slice(&bytes),
                Err(_) => self.env().revert(Error::SerializationError),
            }
        }
        Bytes::from(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::SwapReceipt;
    use odra::host::{Deployer, HostEnv, HostRef};

    const SELL: u64 = 100_000;
    const MIN_OUT: u64 = 198_000;

    struct Fixture {
        env: HostEnv,
        adapter: SettlementAdapterHostRef,
        vault: Address,
        operator: Address,
        recipient: Address,
    }

    fn setup() -> Fixture {
        let env = odra_test::env();
        let operator = env.get_account(0);
        let vault = env.get_account(1);
        let recipient = env.get_account(2);
        env.set_caller(operator);
        let adapter = SettlementAdapter::deploy(
            &env,
            SettlementAdapterInitArgs {
                venue_id: "cspr.trade".to_string(),
                operator,
            },
        );
        Fixture {
            env,
            adapter,
            vault,
            operator,
            recipient,
        }
    }

    /// The vault (caller) escrows a slice, attaching the native sell amount.
    fn escrow(fx: &mut Fixture) -> SwapReceipt {
        fx.env.set_caller(fx.vault);
        fx.adapter.with_tokens(U512::from(SELL)).swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(MIN_OUT),
            fx.recipient,
        )
    }

    /// Reconstruct the exact preimage the contract signs over, off-chain.
    fn settlement_message(
        adapter: &Address,
        escrow_id: u64,
        bought_amount: U512,
        settlement_ref: &Bytes,
        nonce: &Bytes,
        recipient: Address,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&adapter.to_bytes().unwrap());
        buf.extend_from_slice(&escrow_id.to_bytes().unwrap());
        buf.extend_from_slice(&bought_amount.to_bytes().unwrap());
        buf.extend_from_slice(&settlement_ref.to_bytes().unwrap());
        buf.extend_from_slice(&nonce.to_bytes().unwrap());
        buf.extend_from_slice(&recipient.to_bytes().unwrap());
        Bytes::from(buf)
    }

    fn sign_settlement(
        fx: &Fixture,
        escrow_id: u64,
        bought_amount: U512,
        settlement_ref: &Bytes,
        nonce: &Bytes,
    ) -> (PublicKey, Bytes) {
        let addr = fx.adapter.address();
        let msg = settlement_message(
            &addr,
            escrow_id,
            bought_amount,
            settlement_ref,
            nonce,
            fx.recipient,
        );
        let sig = fx.env.sign_message(&msg, &fx.operator);
        let pk = fx.env.public_key(&fx.operator);
        (pk, sig)
    }

    #[test]
    fn swap_escrows_and_returns_non_atomic_receipt() {
        let mut fx = setup();
        let receipt = escrow(&mut fx);
        assert!(!receipt.atomic);
        assert_eq!(receipt.bought_amount, U512::zero());

        let escrow = fx.adapter.get_escrow(0).expect("escrow booked");
        assert_eq!(escrow.sell_amount, U512::from(SELL));
        assert_eq!(escrow.min_out, U512::from(MIN_OUT));
        assert_eq!(escrow.recipient, fx.recipient);
        assert!(!escrow.settled);
    }

    #[test]
    fn venue_id_is_reported() {
        let fx = setup();
        assert_eq!(fx.adapter.venue_id(), "cspr.trade".to_string());
    }

    #[test]
    fn rejects_escrow_amount_mismatch() {
        let fx = setup();
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .with_tokens(U512::from(SELL - 1)) // attach less than declared
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::from(SELL),
                U512::from(MIN_OUT),
                fx.recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::EscrowAmountMismatch.into());
    }

    #[test]
    fn rejects_zero_sell_amount() {
        let mut fx = setup();
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::zero(),
                U512::from(MIN_OUT),
                fx.recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::ZeroSellAmount.into());
    }

    #[test]
    fn operator_attestation_settles_the_escrow() {
        let mut fx = setup();
        escrow(&mut fx);

        let nonce = Bytes::from(vec![9u8; 32]);
        let settlement_ref = Bytes::from(b"deploy-hash-abc".to_vec());
        let bought = U512::from(199_000u64);
        let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);

        // Anyone may submit it; only a valid operator signature authorises it.
        fx.env.set_caller(fx.vault);
        fx.adapter
            .record_settlement(0, bought, settlement_ref, nonce.clone(), pk, sig);

        let escrow = fx.adapter.get_escrow(0).unwrap();
        assert!(escrow.settled);
        assert!(fx.adapter.attestation_used(fx.operator, nonce));
    }

    #[test]
    fn rejects_settlement_below_min_out() {
        let mut fx = setup();
        escrow(&mut fx);
        let nonce = Bytes::from(vec![1u8; 32]);
        let settlement_ref = Bytes::from(b"ref".to_vec());
        let bought = U512::from(MIN_OUT - 1); // below committed floor
        let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::SlippageTooHigh.into());
    }

    #[test]
    fn rejects_tampered_bought_amount() {
        let mut fx = setup();
        escrow(&mut fx);
        let nonce = Bytes::from(vec![2u8; 32]);
        let settlement_ref = Bytes::from(b"ref".to_vec());
        let signed = U512::from(199_000u64);
        let (pk, sig) = sign_settlement(&fx, 0, signed, &settlement_ref, &nonce);
        // Submit a different bought amount than was signed → signature mismatch.
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .try_record_settlement(
                0,
                U512::from(250_000u64),
                settlement_ref,
                nonce,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_wrong_signer() {
        let mut fx = setup();
        escrow(&mut fx);
        let nonce = Bytes::from(vec![3u8; 32]);
        let settlement_ref = Bytes::from(b"ref".to_vec());
        let bought = U512::from(199_000u64);
        let addr = fx.adapter.address();
        let msg = settlement_message(&addr, 0, bought, &settlement_ref, &nonce, fx.recipient);
        // Sign with a non-operator account.
        let sig = fx.env.sign_message(&msg, &fx.recipient);
        let pk = fx.env.public_key(&fx.recipient);
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::NotAuthorizedSigner.into());
    }

    #[test]
    fn rejects_replayed_attestation() {
        let mut fx = setup();
        escrow(&mut fx);
        // Escrow a second slice so the first settlement does not exhaust everything.
        escrow(&mut fx);

        let nonce = Bytes::from(vec![4u8; 32]);
        let settlement_ref = Bytes::from(b"ref".to_vec());
        let bought = U512::from(199_000u64);
        let (pk, sig) = sign_settlement(&fx, 0, bought, &settlement_ref, &nonce);
        fx.env.set_caller(fx.vault);
        fx.adapter.record_settlement(
            0,
            bought,
            settlement_ref.clone(),
            nonce.clone(),
            pk.clone(),
            sig.clone(),
        );
        // Re-submitting the same escrow is blocked by the settled flag.
        let err = fx
            .adapter
            .try_record_settlement(0, bought, settlement_ref, nonce, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::EscrowAlreadySettled.into());
    }

    #[test]
    fn rejects_reused_nonce_across_escrows() {
        let mut fx = setup();
        escrow(&mut fx);
        escrow(&mut fx);

        let nonce = Bytes::from(vec![5u8; 32]);
        let ref0 = Bytes::from(b"ref0".to_vec());
        let bought = U512::from(199_000u64);
        let (pk0, sig0) = sign_settlement(&fx, 0, bought, &ref0, &nonce);
        fx.env.set_caller(fx.vault);
        fx.adapter
            .record_settlement(0, bought, ref0, nonce.clone(), pk0, sig0);

        // Settle escrow 1 reusing the same nonce → replay-protected.
        let ref1 = Bytes::from(b"ref1".to_vec());
        let (pk1, sig1) = sign_settlement(&fx, 1, bought, &ref1, &nonce);
        let err = fx
            .adapter
            .try_record_settlement(1, bought, ref1, nonce, pk1, sig1)
            .unwrap_err();
        assert_eq!(err, Error::AttestationAlreadyUsed.into());
    }

    #[test]
    fn rejects_unknown_escrow() {
        let mut fx = setup();
        let nonce = Bytes::from(vec![6u8; 32]);
        let settlement_ref = Bytes::from(b"ref".to_vec());
        let bought = U512::from(199_000u64);
        let (pk, sig) = sign_settlement(&fx, 7, bought, &settlement_ref, &nonce);
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .try_record_settlement(7, bought, settlement_ref, nonce, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::UnknownEscrow.into());
    }
}
