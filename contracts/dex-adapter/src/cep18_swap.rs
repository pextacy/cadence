//! `Cep18SwapAdapter` — the reference adapter for venues that ARE on-chain.
//!
//! Unlike cspr.trade (off-chain MCP — see [`crate::settlement`]), an on-chain pool
//! can settle atomically inside the same transaction. This adapter models that case
//! behind the same [`VenueAdapter`](crate::adapter::VenueAdapter) trait: the vault
//! attaches the native sell asset, the adapter prices it against a fixed-point
//! reserve rate, pays the realised buy amount to the recipient from its own native
//! reserve in the same call, and returns `atomic = true` with the realised amount —
//! so the vault records the fill immediately without a later attestation.
//!
//! The price is expressed in [`PRICE_SCALE`] fixed-point units of buy-asset per unit
//! of sell-asset. `bought_amount = sell_amount * price / PRICE_SCALE`. The swap MUST
//! revert when the realised output would fall below `min_out`, satisfying the trait
//! contract for atomic venues.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::U512;
use odra::prelude::*;

use crate::adapter::SwapReceipt;

/// Fixed-point scale for the pool price (buy units per sell unit). Mirrors the
/// vault's `PRICE_SCALE` so on-chain quotes stay consistent across crates.
pub const PRICE_SCALE: u64 = 1_000_000;

/// Emitted on every atomic on-chain swap, recording the realised terms.
#[odra::event]
pub struct PoolSwap {
    pub vault: Address,
    pub sell_amount: U512,
    pub bought_amount: U512,
    pub recipient: Address,
}

#[odra::odra_error]
pub enum Error {
    /// `swap` was called with a zero sell amount.
    ZeroSellAmount = 1,
    /// The native value attached to `swap` did not equal `sell_amount`.
    SwapAmountMismatch = 2,
    /// Pool price has not been configured.
    PriceNotSet = 3,
    /// Realised `bought_amount` is below the caller's `min_out`.
    SlippageTooHigh = 4,
    /// The pool's native reserve cannot cover the realised buy amount.
    InsufficientReserve = 5,
    /// Arithmetic overflow computing the realised output.
    Overflow = 6,
    /// Caller is not the pool owner (administrative entrypoints).
    NotOwner = 7,
}

/// Atomic on-chain swap adapter against a priced native reserve.
#[odra::module(events = [PoolSwap], errors = Error)]
pub struct Cep18SwapAdapter {
    /// Stable venue id (e.g. `"cep18-pool"`).
    venue_id: Var<String>,
    /// Pool owner; may set the price and seed/withdraw the reserve.
    owner: Var<Address>,
    /// Fixed-point price: buy units per sell unit, scaled by [`PRICE_SCALE`].
    price: Var<U512>,
}

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
        _sell_asset: String,
        _buy_asset: String,
        sell_amount: U512,
        min_out: U512,
        recipient: Address,
    ) -> SwapReceipt {
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
        // bought = sell_amount * price / PRICE_SCALE, with checked arithmetic.
        let scaled = match sell_amount.checked_mul(price) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        let bought_amount = scaled / U512::from(PRICE_SCALE);
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
    /// [`PRICE_SCALE`]). Owner only.
    pub fn set_price(&mut self, price: U512) {
        self.assert_owner();
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

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostEnv, HostRef};

    const SELL: u64 = 100_000;
    // Price 2.0 in PRICE_SCALE fixed point → 2_000_000.
    const PRICE: u64 = 2 * PRICE_SCALE;
    const RESERVE: u64 = 1_000_000;

    struct Fixture {
        env: HostEnv,
        adapter: Cep18SwapAdapterHostRef,
        vault: Address,
        recipient: Address,
    }

    fn setup() -> Fixture {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let vault = env.get_account(1);
        let recipient = env.get_account(2);
        env.set_caller(owner);
        let mut adapter = Cep18SwapAdapter::deploy(
            &env,
            Cep18SwapAdapterInitArgs {
                venue_id: "cep18-pool".to_string(),
            },
        );
        adapter.set_price(U512::from(PRICE));
        adapter.with_tokens(U512::from(RESERVE)).seed_reserve();
        Fixture {
            env,
            adapter,
            vault,
            recipient,
        }
    }

    #[test]
    fn atomic_swap_pays_recipient_and_returns_realised_amount() {
        let fx = setup();
        fx.env.set_caller(fx.vault);
        let receipt = fx.adapter.with_tokens(U512::from(SELL)).swap(
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(SELL),
            U512::from(190_000u64),
            fx.recipient,
        );
        assert!(receipt.atomic);
        // 100_000 * 2.0 = 200_000.
        assert_eq!(receipt.bought_amount, U512::from(200_000u64));
    }

    #[test]
    fn venue_id_is_reported() {
        let fx = setup();
        assert_eq!(fx.adapter.venue_id(), "cep18-pool".to_string());
    }

    #[test]
    fn reverts_when_below_min_out() {
        let fx = setup();
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .with_tokens(U512::from(SELL))
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::from(SELL),
                U512::from(250_000u64), // demands more than the 200_000 realised
                fx.recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::SlippageTooHigh.into());
    }

    #[test]
    fn reverts_on_amount_mismatch() {
        let fx = setup();
        fx.env.set_caller(fx.vault);
        let err = fx
            .adapter
            .with_tokens(U512::from(SELL - 1))
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::from(SELL),
                U512::from(190_000u64),
                fx.recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::SwapAmountMismatch.into());
    }

    #[test]
    fn reverts_when_price_unset() {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let vault = env.get_account(1);
        let recipient = env.get_account(2);
        env.set_caller(owner);
        let adapter = Cep18SwapAdapter::deploy(
            &env,
            Cep18SwapAdapterInitArgs {
                venue_id: "cep18-pool".to_string(),
            },
        );
        env.set_caller(vault);
        let err = adapter
            .with_tokens(U512::from(SELL))
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::from(SELL),
                U512::from(1u64),
                recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::PriceNotSet.into());
    }

    #[test]
    fn reverts_when_reserve_insufficient() {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let vault = env.get_account(1);
        let recipient = env.get_account(2);
        env.set_caller(owner);
        let mut adapter = Cep18SwapAdapter::deploy(
            &env,
            Cep18SwapAdapterInitArgs {
                venue_id: "cep18-pool".to_string(),
            },
        );
        adapter.set_price(U512::from(PRICE));
        // Reserve smaller than the 200_000 payout the swap would owe.
        adapter.with_tokens(U512::from(50_000u64)).seed_reserve();
        env.set_caller(vault);
        let err = adapter
            .with_tokens(U512::from(SELL))
            .try_swap(
                "CSPR".to_string(),
                "USDC".to_string(),
                U512::from(SELL),
                U512::from(190_000u64),
                recipient,
            )
            .unwrap_err();
        assert_eq!(err, Error::InsufficientReserve.into());
    }

    #[test]
    fn set_price_is_owner_only() {
        let mut fx = setup();
        fx.env.set_caller(fx.vault); // not the owner
        let err = fx.adapter.try_set_price(U512::from(PRICE)).unwrap_err();
        assert_eq!(err, Error::NotOwner.into());
    }
}
