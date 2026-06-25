//! The [`Cep18SwapAdapter`] entrypoints: the atomic `swap`, owner administration,
//! and read-only views. The quote math is delegated to `cadence_common::scale`.

use cadence_common::scale::{apply_price, PRICE_SCALE_1E6};
use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

use super::storage::{Cep18SwapAdapter, Error, PoolSwap};
use crate::adapter::SwapReceipt;

#[odra::module]
impl Cep18SwapAdapter {
    /// Initialise with a venue id and the caller as owner. The price starts unset
    /// and must be configured via [`Self::set_price`] before any swap.
    pub fn init(&mut self, venue_id: String) {
        self.venue_id.set(venue_id);
        self.owner.set(self.env().caller());
        self.price.set(U512::zero());
    }

    // ----- VenueAdapter entrypoints (called cross-contract by the vault) -----

    /// Atomically swap the attached native sell asset into the buy asset, paying the
    /// recipient from the pool reserve in this same call.
    #[odra(payable)]
    pub fn swap(
        &mut self,
        sell_asset: String,
        buy_asset: String,
        sell_amount: U512,
        min_out: U512,
        recipient: Address,
    ) -> SwapReceipt {
        // Parameter names MUST match the `VenueAdapter` trait: Odra dispatches
        // cross-contract args by name, so `_sell_asset`/`_buy_asset` would break
        // trait-ref calls from the vault (MissingArg). This single-pair pool does
        // not branch on the asset symbols.
        let _ = (&sell_asset, &buy_asset);
        if sell_amount.is_zero() {
            self.env().revert(Error::ZeroSellAmount);
        }
        if self.env().attached_value() != sell_amount {
            self.env().revert(Error::SwapAmountMismatch);
        }
        let price = self.price.get_or_default();
        if price.is_zero() {
            self.env().revert(Error::PriceNotSet);
        }
        // bought = sell_amount * price / PRICE_SCALE_1E6, with checked arithmetic.
        // The 1e6 scale is the DEX-adapter scale documented in `cadence_common::scale`;
        // the only reachable failure is a multiply overflow (the divisor is a
        // non-zero constant), which maps to `Error::Overflow` as before.
        let bought_amount = match apply_price(sell_amount, price, PRICE_SCALE_1E6) {
            Ok(v) => v,
            Err(_) => self.env().revert(Error::Overflow),
        };
        if bought_amount < min_out {
            self.env().revert(Error::SlippageTooHigh);
        }
        // The pool must hold enough native reserve to atomically pay out. The vault
        // has already credited `sell_amount` via attached_value this call, so the
        // standing reserve is `self_balance` less the inflow we just received.
        let standing_reserve = self
            .env()
            .self_balance()
            .saturating_sub(self.env().attached_value());
        if standing_reserve < bought_amount {
            self.env().revert(Error::InsufficientReserve);
        }
        self.env().transfer_tokens(&recipient, &bought_amount);

        let vault = self.env().caller();
        self.env().emit_event(PoolSwap {
            vault,
            sell_amount,
            bought_amount,
            recipient,
        });

        SwapReceipt {
            bought_amount,
            settlement_ref: Bytes::from(b"cep18-pool-atomic".to_vec()),
            atomic: true,
        }
    }

    /// Stable venue identifier.
    pub fn venue_id(&self) -> String {
        self.venue_id.get_or_default()
    }

    // ----- owner administration -----

    /// Set the fixed-point pool price (buy units per sell unit, scaled by
    /// [`PRICE_SCALE`](super::PRICE_SCALE)). Owner only.
    pub fn set_price(&mut self, price: U512) {
        self.assert_owner();
        // Reject zero: a zero price reads as "unset" and would brick every swap
        // (Error::PriceNotSet) until re-set — a silent owner foot-gun / DoS.
        if price.is_zero() {
            self.env().revert(Error::PriceNotSet);
        }
        self.price.set(price);
    }

    /// Seed the pool's native buy-asset reserve. Owner only; the attached native
    /// value is retained by the adapter to fund future atomic payouts.
    #[odra(payable)]
    pub fn seed_reserve(&mut self) {
        self.assert_owner();
    }

    // ----- views -----

    pub fn get_price(&self) -> U512 {
        self.price.get_or_default()
    }

    pub fn get_owner(&self) -> Address {
        self.owner.get_or_revert_with(Error::NotOwner)
    }

    pub fn reserve(&self) -> U512 {
        self.env().self_balance()
    }

    fn assert_owner(&self) {
        if self.env().caller() != self.owner.get_or_revert_with(Error::NotOwner) {
            self.env().revert(Error::NotOwner);
        }
    }
}
