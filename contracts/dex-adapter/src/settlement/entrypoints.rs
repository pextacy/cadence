//! The [`SettlementAdapter`] entrypoints: escrow on `swap`, prove the realised fill
//! on `record_settlement`, plus the read-only views.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use super::errors::Error;
use super::events::{EscrowRefunded, SettlementRecorded, SwapIntent};
use super::preimage::settlement_message;
use super::storage::{Escrow, SettlementAdapter};
use crate::adapter::SwapReceipt;

/// How long (ms) an escrow must sit unsettled before its recipient may reclaim it
/// via [`SettlementAdapter::cancel_escrow`]. A generous 24h: settlement is normally
/// minutes, so this only ever fires when the off-chain swap is genuinely abandoned
/// (operator offline, key lost), turning a permanent fund-lock into a recoverable one.
pub const REFUND_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000;

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
                bought_amount: U512::zero(),
                created_at_ms: self.env().get_block_time(),
                refunded: false,
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
    /// against the canonical preimage (see [`super::preimage::settlement_message`]).
    /// Enforces `min_out`, replay-protects on `(operator, nonce)`, releases the
    /// escrowed sell asset to the recipient, and emits [`SettlementRecorded`]. Any
    /// account may submit it — only a valid operator signature authorises it.
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
        // A refunded escrow is terminal: its custody has already been returned, so
        // it can never settle.
        if escrow.refunded {
            self.env().revert(Error::EscrowAlreadyRefunded);
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
        let message = match settlement_message(
            &self.env().self_address(),
            escrow_id,
            bought_amount,
            &settlement_ref,
            &nonce,
            &escrow.recipient,
        ) {
            Ok(m) => m,
            Err(_) => self.env().revert(Error::SerializationError),
        };
        if !self
            .env()
            .verify_signature(&message, &signature, &operator_pk)
        {
            self.env().revert(Error::BadSignature);
        }
        // 6. effects-before-interactions: mark settled, record the proven realised
        //    amount (the vault reads this, not an agent claim), and spend the nonce,
        //    then release the escrowed sell asset to the recipient (the venue/agent leg).
        escrow.settled = true;
        escrow.bought_amount = bought_amount;
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

    /// Reclaim an escrow whose off-chain swap never settled.
    ///
    /// Without this, an escrow can only ever leave the adapter through
    /// [`Self::record_settlement`], which needs a valid operator signature — so if
    /// the operator goes offline or loses its key, the escrowed sell asset is locked
    /// in this contract forever (the vault's `emergency_withdraw` only sweeps the
    /// vault's own balance, never the adapter's). This entrypoint lets the escrow's
    /// **recipient** (the same account the settlement would have paid) reclaim the
    /// custodied sell asset once [`REFUND_TIMEOUT_MS`] has elapsed since the escrow
    /// was booked. It is terminal and mutually exclusive with settlement: a refunded
    /// escrow can never settle, and a settled escrow can never refund.
    pub fn cancel_escrow(&mut self, escrow_id: u64) {
        let mut escrow = match self.escrows.get(&escrow_id) {
            Some(e) => e,
            None => self.env().revert(Error::UnknownEscrow),
        };
        // Terminal-state guards: a settled or already-refunded escrow cannot refund.
        if escrow.settled {
            self.env().revert(Error::EscrowAlreadySettled);
        }
        if escrow.refunded {
            self.env().revert(Error::EscrowAlreadyRefunded);
        }
        // Only the recipient (the beneficiary the settlement would have paid) may
        // reclaim — never an arbitrary caller.
        if self.env().caller() != escrow.recipient {
            self.env().revert(Error::NotRefundRecipient);
        }
        // The refund window must have elapsed since the escrow was booked. Saturate
        // so a pathological `created_at_ms` near u64::MAX can never wrap to a small
        // deadline and open an early refund.
        let deadline = escrow.created_at_ms.saturating_add(REFUND_TIMEOUT_MS);
        if self.env().get_block_time() < deadline {
            self.env().revert(Error::RefundTimeoutNotReached);
        }
        // Effects-before-interactions: mark terminal, then return custody.
        escrow.refunded = true;
        let recipient = escrow.recipient;
        let sell_amount = escrow.sell_amount;
        self.escrows.set(&escrow_id, escrow);
        self.env().transfer_tokens(&recipient, &sell_amount);

        self.env().emit_event(EscrowRefunded {
            escrow_id,
            recipient,
            sell_amount,
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

    /// The operator-attested realised fill for an escrow: `(settled, bought_amount)`.
    ///
    /// `bought_amount` is the amount proven by a verified operator signature in
    /// [`Self::record_settlement`]; `(false, 0)` until then (and for an unknown
    /// escrow). This is the cross-contract read the vault credits to `bought_so_far`
    /// — it never trusts an agent-supplied amount for an off-chain venue.
    pub fn settled_fill(&self, escrow_id: u64) -> (bool, U512) {
        match self.escrows.get(&escrow_id) {
            Some(e) => (e.settled, e.bought_amount),
            None => (false, U512::zero()),
        }
    }
}

/// The cross-contract read surface the vault uses to confirm an escrow's
/// operator-attested fill before crediting it.
///
/// Declared `#[odra::external_contract]` so the vault can build a
/// `SettlementProofContractRef::new(env, adapter_addr)` from a resolved `Address`
/// and read `settled_fill` without depending on the concrete adapter type. The
/// dispatch is by entrypoint name, so the method name here MUST match the
/// `settled_fill` entrypoint above.
#[odra::external_contract]
pub trait SettlementProof {
    /// `(settled, bought_amount)` — the operator-attested realised fill.
    fn settled_fill(&self, escrow_id: u64) -> (bool, U512);
}
