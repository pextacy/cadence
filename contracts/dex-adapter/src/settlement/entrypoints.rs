//! The [`SettlementAdapter`] entrypoints: escrow on `swap`, prove the realised fill
//! on `record_settlement`, plus the read-only views.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use super::errors::Error;
use super::events::{SettlementRecorded, SwapIntent};
use super::preimage::settlement_message;
use super::storage::{Escrow, SettlementAdapter};
use crate::adapter::SwapReceipt;

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
}
