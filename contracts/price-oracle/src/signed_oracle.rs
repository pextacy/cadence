//! A signed-price-feed verifier — Cadence's on-chain price oracle.
//!
//! Casper exposes no native price oracle, and (like the x402 token, see
//! `contracts/x402-token/src/token.rs:7-18`) it has **no keccak256/ecrecover** —
//! only `env().hash()` (blake2b) and `env().verify_signature(message, signature,
//! public_key)` which verifies against a *supplied* `PublicKey`. So this oracle
//! authorizes feed updates with Casper's native scheme, reusing the exact
//! signature-verification + replay-protection + canonical `ToBytes` preimage
//! pattern proven in `x402-token`.
//!
//! ## Shape
//!
//! An authorized **oracle operator** (its `PublicKey` registered at `init`) posts
//! a price for a trading `pair` by signing the canonical preimage
//! `self_address ‖ pair ‖ price ‖ timestamp_ms ‖ round` (see
//! [`crate::preimage`]) with their Casper key. `submit_price` (callable by any
//! relayer — the operator spends no gas) verifies:
//!   1. the supplied key hashes to the registered operator account,
//!   2. the signature verifies against the reconstructed preimage,
//!   3. the `round` strictly increases per pair (rejects stale/out-of-order),
//!   4. the `timestamp_ms` is not in the future and not older than the last.
//!
//! It then stores the latest [`PriceData`] for the pair and emits
//! [`PriceUpdated`](crate::events::PriceUpdated).
//!
//! Consumers (e.g. the execution vault band-checking `quoted_out`) call
//! [`SignedPriceOracle::latest_price`], a staleness-checked read that **reverts**
//! if the freshest price for the pair is older than `max_staleness_ms`. This
//! implements the [`OracleAdapter`](crate::types::OracleAdapter) external-contract
//! interface so the vault and the aggregator can resolve a
//! `OracleAdapterContractRef` and cross-check prices.

use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

use crate::errors::Error;
use crate::events::{OperatorSet, PriceUpdated};
use crate::preimage::price_message;
use crate::types::PriceData;

/// A signed-price-feed oracle: one authorized operator key, per-pair latest price.
#[odra::module(events = [PriceUpdated, OperatorSet], errors = Error)]
pub struct SignedPriceOracle {
    /// The account hash of the authorized oracle operator.
    operator: Var<Address>,
    /// Maximum age (ms) a stored price may have before reads revert as stale.
    max_staleness_ms: Var<u64>,
    /// Latest accepted price per pair.
    prices: Mapping<String, PriceData>,
    /// Last accepted round per pair (enforces strict monotonicity).
    last_round: Mapping<String, u64>,
    /// Set once `init` runs, to make re-initialisation revert.
    initialised: Var<bool>,
}

#[odra::module]
impl SignedPriceOracle {
    /// Register the authorized oracle operator (by `PublicKey`, stored as the
    /// derived account `Address`) and the read staleness bound.
    ///
    /// `max_staleness_ms` must be positive — otherwise every read would revert.
    pub fn init(&mut self, operator_pk: PublicKey, max_staleness_ms: u64) {
        if self.initialised.get_or_default() {
            self.env().revert(Error::AlreadyInitialised);
        }
        if max_staleness_ms == 0 {
            self.env().revert(Error::ZeroStaleness);
        }
        let operator = Address::from(operator_pk);
        self.operator.set(operator);
        self.max_staleness_ms.set(max_staleness_ms);
        self.initialised.set(true);
        self.env().emit_event(OperatorSet {
            operator,
            max_staleness_ms,
        });
    }

    /// The registered operator account.
    pub fn operator(&self) -> Address {
        self.operator.get_or_revert_with(Error::AlreadyInitialised)
    }

    /// The configured read staleness bound (ms).
    pub fn max_staleness_ms(&self) -> u64 {
        self.max_staleness_ms.get_or_default()
    }

    /// The last accepted round for `pair` (0 if none yet).
    pub fn last_round(&self, pair: String) -> u64 {
        self.last_round.get_or_default(&pair)
    }

    /// Raw stored price for `pair` without the staleness gate. `None` if unset.
    /// Use [`latest_price`](Self::latest_price) for consumer reads.
    pub fn get_price(&self, pair: String) -> Option<PriceData> {
        self.prices.get(&pair)
    }

    /// Submit a signed price for `pair`. Any account may relay this; the operator
    /// signs the canonical preimage but spends no gas.
    ///
    /// Reverts unless `operator_pk` hashes to the registered operator, the
    /// signature verifies, `round` strictly exceeds the last accepted round for
    /// the pair, and `timestamp_ms` is fresh (not future, strictly newer than the
    /// stored timestamp).
    #[allow(clippy::too_many_arguments)]
    pub fn submit_price(
        &mut self,
        pair: String,
        price: U512,
        timestamp_ms: u64,
        round: u64,
        operator_pk: PublicKey,
        signature: Bytes,
    ) {
        // 1. price sanity.
        if price.is_zero() {
            self.env().revert(Error::ZeroPrice);
        }
        // 2. the supplied key must be the registered operator's key.
        let operator = self.operator.get_or_revert_with(Error::NotAuthorizedSigner);
        if Address::from(operator_pk.clone()) != operator {
            self.env().revert(Error::NotAuthorizedSigner);
        }
        // 3. the signature must verify against the canonical preimage.
        let message = match price_message(
            &self.env().self_address(),
            &pair,
            price,
            timestamp_ms,
            round,
        ) {
            Some(m) => m,
            None => self.env().revert(Error::SerializationError),
        };
        if !self
            .env()
            .verify_signature(&message, &signature, &operator_pk)
        {
            self.env().revert(Error::BadSignature);
        }
        // 4. round must strictly increase (rejects replay / out-of-order).
        let prev_round = self.last_round.get_or_default(&pair);
        if round <= prev_round && (prev_round != 0 || self.prices.get(&pair).is_some()) {
            self.env().revert(Error::StaleRound);
        }
        // 5. timestamp must not be in the future, and must be strictly newer.
        let now = self.env().get_block_time();
        if timestamp_ms > now {
            self.env().revert(Error::TimestampInFuture);
        }
        if let Some(existing) = self.prices.get(&pair) {
            if timestamp_ms <= existing.timestamp_ms {
                self.env().revert(Error::StaleTimestamp);
            }
        }
        // 6. accept: store latest and bump the round.
        self.prices.set(
            &pair,
            PriceData {
                price,
                timestamp_ms,
                round,
            },
        );
        self.last_round.set(&pair, round);
        self.env().emit_event(PriceUpdated {
            pair,
            price,
            timestamp_ms,
            round,
        });
    }

    /// Latest accepted price for `pair`, fixed-point at
    /// [`PRICE_SCALE`](crate::types::PRICE_SCALE).
    ///
    /// Reverts [`Error::NoPrice`] if the pair has never been priced, or
    /// [`Error::StalePrice`] if the freshest price is older than the configured
    /// `max_staleness_ms`. This is the consumer read the vault and aggregator call
    /// via [`OracleAdapter::latest_price`](crate::types::OracleAdapter).
    pub fn latest_price(&self, pair: String) -> PriceData {
        let data = match self.prices.get(&pair) {
            Some(d) => d,
            None => self.env().revert(Error::NoPrice),
        };
        let now = self.env().get_block_time();
        let max_staleness = self.max_staleness_ms.get_or_default();
        // `now` is always >= a previously-accepted timestamp (submit rejects
        // future timestamps), so this subtraction never underflows.
        let age = now.saturating_sub(data.timestamp_ms);
        if age > max_staleness {
            self.env().revert(Error::StalePrice);
        }
        data
    }

    /// Rotate the authorized operator key. Operator-only (the current operator
    /// must be the caller), so a compromised feed key can be revoked without
    /// redeploying the oracle.
    pub fn rotate_operator(&mut self, new_operator_pk: PublicKey) {
        let current = self.operator.get_or_revert_with(Error::NotOperator);
        if self.env().caller() != current {
            self.env().revert(Error::NotOperator);
        }
        let operator = Address::from(new_operator_pk);
        self.operator.set(operator);
        let max_staleness_ms = self.max_staleness_ms.get_or_default();
        self.env().emit_event(OperatorSet {
            operator,
            max_staleness_ms,
        });
    }
}
