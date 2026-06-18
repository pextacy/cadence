//! Vault lifecycle entrypoints: `init` (store + verify the mandate) and `fund`.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use super::errors::Error;
use super::events::{MandateInitialised, MandateVerified, VaultFunded};
use super::status::Status;
use super::storage::ExecutionVault;

impl ExecutionVault {
    /// Store the signed mandate and its decoded limits. Called once by the
    /// treasury. The caller becomes the treasury identity. `agent` is the
    /// account-abstraction identity later authorised to call `execute_slice`.
    ///
    /// The mandate's authority is established by a **Casper-native** signature:
    /// `treasury_public_key` must hash to the `init` caller, and `casper_signature`
    /// must verify against the canonical [`mandate_message`](ExecutionVault::mandate_message)
    /// preimage that encodes every enforced limit plus `mandate_nonce`. Either
    /// failure reverts, so the on-chain limits are provably the ones the treasury
    /// signed. The EIP-712 `mandate_digest` is also stored/emitted for off-chain
    /// human re-derivation.
    ///
    /// Times are in **milliseconds** to match the Casper block time. Prices are
    /// fixed-point with [`PRICE_SCALE`](super::constants::PRICE_SCALE); pass `0` for
    /// an unset floor/ceiling.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn init_impl(
        &mut self,
        agent: Address,
        mandate_digest: Bytes,
        signature: Bytes,
        treasury_public_key: PublicKey,
        casper_signature: Bytes,
        mandate_nonce: Bytes,
        sell_asset: String,
        buy_asset: String,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: Vec<String>,
        venue_addresses: Vec<Address>,
    ) {
        // Odra constructors are one-shot at install time; the framework rejects any
        // attempt to re-invoke `init`, so no in-body re-entry guard is needed.
        if mandate_digest.len() != 32 {
            self.env().revert(Error::BadDigestLength);
        }
        if total_sell.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        // Each allowlisted venue must carry exactly one destination address so the
        // spend entrypoint can resolve where funds go without trusting the caller.
        if venues.len() != venue_addresses.len() {
            self.env().revert(Error::VenueConfigMismatch);
        }

        // The funding sender becomes the treasury; the supplied public key MUST be
        // that account's key (mirrors x402 token.rs:188).
        let treasury = self.env().caller();
        if Address::from(treasury_public_key.clone()) != treasury {
            self.env().revert(Error::NotAuthorizedSigner);
        }

        // Reconstruct the canonical preimage the treasury signed off-chain and
        // verify the Casper-native signature against it (mirrors token.rs:205-208).
        // Built and checked BEFORE any storage write, so a forged or mismatched
        // authorization never mutates state.
        let preimage = self.mandate_message(
            agent,
            treasury,
            &sell_asset,
            &buy_asset,
            total_sell,
            end_time_ms,
            max_slippage_bps,
            price_floor,
            price_ceiling,
            &venues,
            &venue_addresses,
            &mandate_nonce,
        );
        if !self
            .env()
            .verify_signature(&preimage, &casper_signature, &treasury_public_key)
        {
            self.env().revert(Error::BadSignature);
        }
        // blake2b hash of the verified preimage, for the audit event.
        let preimage_hash = Bytes::from(self.env().hash(preimage.as_slice()).to_vec());

        self.treasury.set(treasury);
        self.agent.set(agent);
        self.treasury_public_key.set(treasury_public_key);
        self.mandate_digest.set(mandate_digest.clone());
        self.signature.set(signature.clone());
        self.casper_signature.set(casper_signature);
        self.mandate_nonce.set(mandate_nonce.clone());
        self.sell_asset.set(sell_asset.clone());
        self.buy_asset.set(buy_asset.clone());
        self.total_sell.set(total_sell);
        self.end_time_ms.set(end_time_ms);
        self.max_slippage_bps.set(max_slippage_bps);
        self.price_floor.set(price_floor);
        self.price_ceiling.set(price_ceiling);
        for (venue, addr) in venues.iter().zip(venue_addresses.iter()) {
            self.venue_allowlist.set(venue, true);
            self.venue_addr.set(venue, *addr);
        }
        self.venue_ids.set(venues);
        self.venue_addr_list.set(venue_addresses);
        self.sold_so_far.set(U512::zero());
        self.bought_so_far.set(U512::zero());
        self.slice_count.set(0);
        self.status.set(Status::Funded);

        self.env().emit_event(MandateInitialised {
            treasury,
            agent,
            mandate_digest,
            signature,
            sell_asset,
            buy_asset,
            total_sell,
            end_time_ms,
            max_slippage_bps,
        });
        self.env().emit_event(MandateVerified {
            treasury,
            mandate_nonce,
            preimage_hash,
        });
    }

    /// Treasury funds the vault with the sell asset (native CSPR). The attached
    /// value must equal the mandate total. Moves the vault to `Active`.
    pub(super) fn fund_impl(&mut self) {
        self.assert_treasury();
        if self.read_status() != Status::Funded {
            self.env().revert(Error::NotFunded);
        }
        let amount = self.env().attached_value();
        if amount != self.total_sell.get_or_default() {
            self.env().revert(Error::FundingMismatch);
        }
        self.status.set(Status::Active);
        self.env().emit_event(VaultFunded {
            amount,
            balance: self.env().self_balance(),
        });
    }
}
