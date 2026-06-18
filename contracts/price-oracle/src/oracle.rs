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
//! `self_address ‖ pair ‖ price ‖ timestamp_ms ‖ round` with their Casper key.
//! `submit_price` (callable by any relayer — the operator spends no gas) verifies:
//!   1. the supplied key hashes to the registered operator account,
//!   2. the signature verifies against the reconstructed preimage,
//!   3. the `round` strictly increases per pair (rejects stale/out-of-order),
//!   4. the `timestamp_ms` is not in the future and not older than the last.
//! It then stores the latest [`PriceData`] for the pair and emits [`PriceUpdated`].
//!
//! Consumers (e.g. the execution vault band-checking `quoted_out`) call
//! [`SignedPriceOracle::latest_price`], a staleness-checked read that **reverts**
//! if the freshest price for the pair is older than `max_staleness_ms`. This
//! matches the `OracleAdapter::latest_price` external-contract interface so the
//! vault can resolve a `SignedPriceOracleContractRef` and cross-check prices.
//!
//! Prices are fixed-point with [`PRICE_SCALE`] decimals and use `U512` to match
//! the vault's amount type.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

/// Fixed-point scale for prices: a `price` of `1 * PRICE_SCALE` means 1.0 buy per
/// sell unit. Keep in sync with the vault's `PRICE_SCALE` so band checks line up.
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// A latest accepted price for a pair, fixed-point at [`PRICE_SCALE`].
///
/// Mirrors the `OracleAdapter::PriceData` interface so the vault's external
/// contract reference deserializes it identically.
#[odra::odra_type]
pub struct PriceData {
    /// Price in fixed-point base units (`actual_price * PRICE_SCALE`).
    pub price: U512,
    /// Casper block time (milliseconds) the operator stamped on the round.
    pub timestamp_ms: u64,
    /// Strictly-monotonic per-pair sequence number.
    pub round: u64,
}

/// Emitted when a signed price is accepted and becomes the latest for its pair.
#[odra::event]
pub struct PriceUpdated {
    pub pair: String,
    pub price: U512,
    pub timestamp_ms: u64,
    pub round: u64,
}

/// Emitted at `init` (and on rotation) recording the authorized operator account.
#[odra::event]
pub struct OperatorSet {
    pub operator: Address,
    pub max_staleness_ms: u64,
}

#[odra::odra_error]
pub enum Error {
    /// `init` was called more than once.
    AlreadyInitialised = 1,
    /// The supplied public key does not hash to the registered operator account.
    NotAuthorizedSigner = 2,
    /// The signature does not verify against the price preimage.
    BadSignature = 3,
    /// Failed to serialize the price preimage.
    SerializationError = 4,
    /// `round` did not strictly increase for the pair (stale / replayed update).
    StaleRound = 5,
    /// `timestamp_ms` is older than (or equal to) the stored timestamp.
    StaleTimestamp = 6,
    /// `timestamp_ms` is in the future relative to block time.
    TimestampInFuture = 7,
    /// A zero price was supplied where a positive value is required.
    ZeroPrice = 8,
    /// `latest_price` was requested for a pair with no accepted price yet.
    NoPrice = 9,
    /// The freshest price for the pair is older than `max_staleness_ms`.
    StalePrice = 10,
    /// `max_staleness_ms` was zero at init (a price could never be read).
    ZeroStaleness = 11,
    /// Operator rotation attempted by a non-operator caller.
    NotOperator = 12,
}

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
        self.env().emit_event(OperatorSet { operator, max_staleness_ms });
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
        let message = self.price_message(&pair, price, timestamp_ms, round);
        if !self.env().verify_signature(&message, &signature, &operator_pk) {
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
        self.prices.set(&pair, PriceData { price, timestamp_ms, round });
        self.last_round.set(&pair, round);
        self.env().emit_event(PriceUpdated { pair, price, timestamp_ms, round });
    }

    /// Latest accepted price for `pair`, fixed-point at [`PRICE_SCALE`].
    ///
    /// Reverts [`Error::NoPrice`] if the pair has never been priced, or
    /// [`Error::StalePrice`] if the freshest price is older than the configured
    /// `max_staleness_ms`. This is the consumer read the vault calls via
    /// `OracleAdapter::latest_price`.
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
        self.env().emit_event(OperatorSet { operator, max_staleness_ms });
    }

    // ----- internal helpers (private — never exposed as entrypoints) -----

    /// The canonical price preimage the operator signs and the contract verifies:
    /// the contract's own address (binds the price to *this* oracle, preventing
    /// cross-contract replay) followed by every price field, each in Casper
    /// `ToBytes` encoding. Field order is frozen: `self_address ‖ pair ‖ price ‖
    /// timestamp_ms ‖ round`. An off-chain signer MUST reproduce this exact
    /// buffer (see `mandate/src/settlementAttest.ts` discipline).
    fn price_message(&self, pair: &str, price: U512, timestamp_ms: u64, round: u64) -> Bytes {
        let parts: [Result<Vec<u8>, _>; 5] = [
            self.env().self_address().to_bytes(),
            pair.to_bytes(),
            price.to_bytes(),
            timestamp_ms.to_bytes(),
            round.to_bytes(),
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
    use odra::host::{Deployer, HostEnv};

    const MAX_STALENESS_MS: u64 = 60_000;
    const PAIR: &str = "CSPR/USDC";

    struct Fixture {
        env: HostEnv,
        oracle: SignedPriceOracleHostRef,
        operator: Address,
        relayer: Address,
        stranger: Address,
    }

    fn setup() -> Fixture {
        let env = odra_test::env();
        let deployer = env.get_account(0);
        let operator = env.get_account(1);
        let relayer = env.get_account(2);
        let stranger = env.get_account(3);
        env.set_caller(deployer);
        let oracle = SignedPriceOracle::deploy(
            &env,
            SignedPriceOracleInitArgs {
                operator_pk: env.public_key(&operator),
                max_staleness_ms: MAX_STALENESS_MS,
            },
        );
        Fixture { env, oracle, operator, relayer, stranger }
    }

    /// Reconstruct the exact preimage the contract signs over, off-chain.
    fn price_message(
        oracle: &Address,
        pair: &str,
        price: U512,
        timestamp_ms: u64,
        round: u64,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&oracle.to_bytes().unwrap());
        buf.extend_from_slice(&pair.to_bytes().unwrap());
        buf.extend_from_slice(&price.to_bytes().unwrap());
        buf.extend_from_slice(&timestamp_ms.to_bytes().unwrap());
        buf.extend_from_slice(&round.to_bytes().unwrap());
        Bytes::from(buf)
    }

    /// Build a valid (publicKey, signature) for a price from the operator.
    fn sign_price(
        fx: &Fixture,
        signer: &Address,
        pair: &str,
        price: U512,
        timestamp_ms: u64,
        round: u64,
    ) -> (PublicKey, Bytes) {
        let oracle_addr = fx.oracle.address();
        let msg = price_message(&oracle_addr, pair, price, timestamp_ms, round);
        let sig = fx.env.sign_message(&msg, signer);
        let pk = fx.env.public_key(signer);
        (pk, sig)
    }

    #[test]
    fn accepts_a_signed_price_and_reads_it_back() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_250_000_000u64); // 1.25 at PRICE_SCALE
        let ts = 5_000u64;
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, ts, 1);

        // A relayer (not the operator) submits the signed update.
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price(PAIR.to_string(), price, ts, 1, pk, sig);

        let data = fx.oracle.latest_price(PAIR.to_string());
        assert_eq!(data.price, price);
        assert_eq!(data.timestamp_ms, ts);
        assert_eq!(data.round, 1);
        assert_eq!(fx.oracle.last_round(PAIR.to_string()), 1);
    }

    #[test]
    fn rejects_zero_price() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, U512::zero(), 5_000, 1);
        fx.env.set_caller(fx.relayer);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), U512::zero(), 5_000, 1, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::ZeroPrice.into());
    }

    #[test]
    fn rejects_unauthorized_signer() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_000_000_000u64);
        // Signed by a stranger, not the registered operator.
        let (pk, sig) = sign_price(&fx, &fx.stranger, PAIR, price, 5_000, 1);
        fx.env.set_caller(fx.relayer);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::NotAuthorizedSigner.into());
    }

    #[test]
    fn rejects_tampered_price() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let signed_price = U512::from(1_000_000_000u64);
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, signed_price, 5_000, 1);
        // Submit a different price than was signed → signature no longer matches.
        fx.env.set_caller(fx.relayer);
        let err = fx
            .oracle
            .try_submit_price(
                PAIR.to_string(),
                U512::from(2_000_000_000u64),
                5_000,
                1,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_out_of_order_round() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_000_000_000u64);
        let (pk1, sig1) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 5);
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price(PAIR.to_string(), price, 5_000, 5, pk1, sig1);

        // Round 4 < 5 → rejected even with a fresher timestamp.
        let (pk2, sig2) = sign_price(&fx, &fx.operator, PAIR, price, 6_000, 4);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 6_000, 4, pk2, sig2)
            .unwrap_err();
        assert_eq!(err, Error::StaleRound.into());
    }

    #[test]
    fn rejects_equal_round_replay() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_000_000_000u64);
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
        fx.env.set_caller(fx.relayer);
        fx.oracle
            .submit_price(PAIR.to_string(), price, 5_000, 1, pk.clone(), sig.clone());
        // Same round replayed.
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::StaleRound.into());
    }

    #[test]
    fn rejects_future_timestamp() {
        let mut fx = setup();
        fx.env.advance_block_time(5_000);
        let price = U512::from(1_000_000_000u64);
        // timestamp 9_000 > block time 5_000.
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 9_000, 1);
        fx.env.set_caller(fx.relayer);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 9_000, 1, pk, sig)
            .unwrap_err();
        assert_eq!(err, Error::TimestampInFuture.into());
    }

    #[test]
    fn rejects_non_increasing_timestamp() {
        let mut fx = setup();
        fx.env.advance_block_time(20_000);
        let price = U512::from(1_000_000_000u64);
        let (pk1, sig1) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 1);
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price(PAIR.to_string(), price, 10_000, 1, pk1, sig1);
        // Higher round but a stale (equal) timestamp → rejected.
        let (pk2, sig2) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 2);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 10_000, 2, pk2, sig2)
            .unwrap_err();
        assert_eq!(err, Error::StaleTimestamp.into());
    }

    #[test]
    fn latest_price_reverts_when_unset() {
        let fx = setup();
        let err = fx.oracle.try_latest_price(PAIR.to_string()).unwrap_err();
        assert_eq!(err, Error::NoPrice.into());
    }

    #[test]
    fn latest_price_reverts_when_stale() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_000_000_000u64);
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price(PAIR.to_string(), price, 5_000, 1, pk, sig);
        // Age now 5_000ms; advance well past the 60_000ms bound.
        fx.env.advance_block_time(MAX_STALENESS_MS + 10_000);
        let err = fx.oracle.try_latest_price(PAIR.to_string()).unwrap_err();
        assert_eq!(err, Error::StalePrice.into());
    }

    #[test]
    fn fresh_within_bound_is_readable() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let price = U512::from(1_000_000_000u64);
        // Stamp the price at the current block time, then advance to just under
        // the staleness edge so the read is still fresh.
        let (pk, sig) = sign_price(&fx, &fx.operator, PAIR, price, 10_000, 1);
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price(PAIR.to_string(), price, 10_000, 1, pk, sig);
        fx.env.advance_block_time(MAX_STALENESS_MS - 1);
        let data = fx.oracle.latest_price(PAIR.to_string());
        assert_eq!(data.price, price);
    }

    #[test]
    fn rotate_operator_changes_authorized_signer() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let new_op = fx.env.get_account(4);

        // Only the current operator may rotate.
        fx.env.set_caller(fx.stranger);
        let err = fx
            .oracle
            .try_rotate_operator(fx.env.public_key(&new_op))
            .unwrap_err();
        assert_eq!(err, Error::NotOperator.into());

        fx.env.set_caller(fx.operator);
        fx.oracle.rotate_operator(fx.env.public_key(&new_op));
        assert_eq!(fx.oracle.operator(), new_op);

        // Old operator's signature is now rejected.
        let price = U512::from(1_000_000_000u64);
        let (old_pk, old_sig) = sign_price(&fx, &fx.operator, PAIR, price, 5_000, 1);
        fx.env.set_caller(fx.relayer);
        let err = fx
            .oracle
            .try_submit_price(PAIR.to_string(), price, 5_000, 1, old_pk, old_sig)
            .unwrap_err();
        assert_eq!(err, Error::NotAuthorizedSigner.into());

        // New operator's signature is accepted.
        let (new_pk, new_sig) = sign_price(&fx, &new_op, PAIR, price, 5_000, 1);
        fx.oracle
            .submit_price(PAIR.to_string(), price, 5_000, 1, new_pk, new_sig);
        assert_eq!(fx.oracle.latest_price(PAIR.to_string()).round, 1);
    }

    #[test]
    fn rejects_double_init() {
        let mut fx = setup();
        // Odra blocks calling a constructor entrypoint a second time at the VM
        // level (InvalidContext), which is the actual double-init protection;
        // the in-contract AlreadyInitialised guard is defence-in-depth.
        let result = fx
            .oracle
            .try_init(fx.env.public_key(&fx.operator), MAX_STALENESS_MS);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_zero_staleness_init() {
        let env = odra_test::env();
        let deployer = env.get_account(0);
        let operator = env.get_account(1);
        env.set_caller(deployer);
        let result = SignedPriceOracle::try_deploy(
            &env,
            SignedPriceOracleInitArgs {
                operator_pk: env.public_key(&operator),
                max_staleness_ms: 0,
            },
        );
        assert_eq!(result.err(), Some(Error::ZeroStaleness.into()));
    }

    #[test]
    fn supports_multiple_independent_pairs() {
        let mut fx = setup();
        fx.env.advance_block_time(10_000);
        let p1 = U512::from(1_000_000_000u64);
        let p2 = U512::from(2_500_000_000u64);
        let (pk1, sig1) = sign_price(&fx, &fx.operator, "CSPR/USDC", p1, 5_000, 1);
        let (pk2, sig2) = sign_price(&fx, &fx.operator, "BTC/USDC", p2, 5_000, 1);
        fx.env.set_caller(fx.relayer);
        fx.oracle.submit_price("CSPR/USDC".to_string(), p1, 5_000, 1, pk1, sig1);
        fx.oracle.submit_price("BTC/USDC".to_string(), p2, 5_000, 1, pk2, sig2);
        assert_eq!(fx.oracle.latest_price("CSPR/USDC".to_string()).price, p1);
        assert_eq!(fx.oracle.latest_price("BTC/USDC".to_string()).price, p2);
    }
}
