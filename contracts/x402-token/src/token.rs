//! An x402-payable CEP-18 token: a standard fungible token plus a **gasless,
//! signature-authorized transfer** (`transfer_with_authorization`). This is the
//! on-chain counterpart of the x402 pay-per-call flow — a payer signs an
//! authorization off-chain and any relayer can submit it, so the payer spends no
//! gas and never sends the transaction itself.
//!
//! ## Why Casper-native signatures, not Ethereum EIP-712
//!
//! The off-chain x402 reference (`agent/src/clients/x402.ts`) builds an Ethereum
//! EIP-712 `TransferAuthorization` (keccak256 + secp256k1 *recover*) for the
//! external CSPR.cloud facilitator. A Casper contract cannot reproduce that: Odra
//! exposes `env().hash()` (**blake2b**, not keccak256) and
//! `env().verify_signature(message, signature, public_key)` (verify against a
//! *supplied* public key, not ecrecover). So this contract authorizes with
//! Casper's native scheme: the payer signs the canonical authorization preimage
//! with their Casper key, supplies their `PublicKey`, and the contract verifies
//! the signature and checks the key hashes to `from`. Same x402 semantics
//! (gasless, replay-protected, time-bounded), Casper-native crypto.

use crate::errors::Error;
use crate::preimage::authorization_message;
use odra::casper_types::bytesrepr::Bytes;
use odra::casper_types::{PublicKey, U256};
use odra::prelude::*;

#[odra::event]
pub struct Transfer {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
}

#[odra::event]
pub struct Approval {
    pub owner: Address,
    pub spender: Address,
    pub amount: U256,
}

#[odra::event]
pub struct Mint {
    pub to: Address,
    pub amount: U256,
}

/// Emitted when a `transfer_with_authorization` settles, linking the spent nonce.
#[odra::event]
pub struct AuthorizationUsed {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub nonce: Bytes,
}

/// A CEP-18 fungible token with an x402-style gasless authorized transfer.
#[odra::module(events = [Transfer, Approval, Mint, AuthorizationUsed], errors = Error)]
pub struct X402Token {
    name: Var<String>,
    symbol: Var<String>,
    decimals: Var<u8>,
    total_supply: Var<U256>,
    owner: Var<Address>,
    balances: Mapping<Address, U256>,
    allowances: Mapping<(Address, Address), U256>,
    /// Spent authorization nonces, keyed by `(from, nonce)` for replay protection.
    used_authorizations: Mapping<(Address, Bytes), bool>,
}

#[odra::module]
impl X402Token {
    /// Initialise token metadata, set the caller as owner (mint authority), and
    /// mint `initial_supply` to the caller.
    pub fn init(&mut self, name: String, symbol: String, decimals: u8, initial_supply: U256) {
        let caller = self.env().caller();
        self.name.set(name);
        self.symbol.set(symbol);
        self.decimals.set(decimals);
        self.owner.set(caller);
        self.total_supply.set(U256::zero());
        if !initial_supply.is_zero() {
            self.mint_to(caller, initial_supply);
        }
    }

    pub fn name(&self) -> String {
        self.name.get_or_default()
    }
    pub fn symbol(&self) -> String {
        self.symbol.get_or_default()
    }
    pub fn decimals(&self) -> u8 {
        self.decimals.get_or_default()
    }
    pub fn total_supply(&self) -> U256 {
        self.total_supply.get_or_default()
    }
    pub fn owner(&self) -> Address {
        self.owner.get_or_revert_with(Error::NotOwner)
    }
    pub fn balance_of(&self, address: Address) -> U256 {
        self.balances.get_or_default(&address)
    }
    pub fn allowance(&self, owner: Address, spender: Address) -> U256 {
        self.allowances.get_or_default(&(owner, spender))
    }
    /// Whether a `(from, nonce)` authorization has already been spent.
    pub fn authorization_used(&self, from: Address, nonce: Bytes) -> bool {
        self.used_authorizations.get_or_default(&(from, nonce))
    }

    /// Transfer `amount` from the caller to `recipient`.
    pub fn transfer(&mut self, recipient: Address, amount: U256) {
        let caller = self.env().caller();
        self.do_transfer(caller, recipient, amount);
    }

    /// Approve `spender` to spend up to `amount` of the caller's balance.
    pub fn approve(&mut self, spender: Address, amount: U256) {
        let owner = self.env().caller();
        self.allowances.set(&(owner, spender), amount);
        self.env().emit_event(Approval {
            owner,
            spender,
            amount,
        });
    }

    /// Transfer `amount` from `owner` to `recipient`, spending the caller's
    /// allowance granted by `owner`.
    pub fn transfer_from(&mut self, owner: Address, recipient: Address, amount: U256) {
        let spender = self.env().caller();
        let current = self.allowances.get_or_default(&(owner, spender));
        if current < amount {
            self.env().revert(Error::InsufficientAllowance);
        }
        self.allowances.set(&(owner, spender), current - amount);
        self.do_transfer(owner, recipient, amount);
    }

    /// Mint `amount` into `to`. Owner only.
    pub fn mint(&mut self, to: Address, amount: U256) {
        if self.env().caller() != self.owner.get_or_revert_with(Error::NotOwner) {
            self.env().revert(Error::NotOwner);
        }
        self.mint_to(to, amount);
    }

    /// Gasless, signature-authorized transfer (the x402 settlement primitive).
    ///
    /// `from` authorizes moving `value` to `to`, valid within
    /// `[valid_after_ms, valid_before_ms]` (Casper block time, milliseconds), bound
    /// to a unique `nonce`. `public_key` must hash to `from`, and `signature` must
    /// verify against the canonical preimage (see [`authorization_message`]). Any
    /// account may submit this call — the payer signs but spends no gas.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer_with_authorization(
        &mut self,
        from: Address,
        to: Address,
        value: U256,
        valid_after_ms: u64,
        valid_before_ms: u64,
        nonce: Bytes,
        public_key: PublicKey,
        signature: Bytes,
    ) {
        // 1. the supplied key must be the `from` account's key.
        if Address::from(public_key.clone()) != from {
            self.env().revert(Error::NotAuthorizedSigner);
        }
        // 2. within the validity window.
        let now = self.env().get_block_time();
        if now < valid_after_ms {
            self.env().revert(Error::AuthorizationNotYetValid);
        }
        if now > valid_before_ms {
            self.env().revert(Error::AuthorizationExpired);
        }
        // 3. not already used.
        let nonce_key = (from, nonce.clone());
        if self.used_authorizations.get_or_default(&nonce_key) {
            self.env().revert(Error::AuthorizationAlreadyUsed);
        }
        // 4. the signature must verify against the canonical preimage.
        let message = match authorization_message(
            self.env().self_address(),
            from,
            to,
            value,
            valid_after_ms,
            valid_before_ms,
            &nonce,
        ) {
            Ok(message) => message,
            Err(_) => self.env().revert(Error::SerializationError),
        };
        if !self
            .env()
            .verify_signature(&message, &signature, &public_key)
        {
            self.env().revert(Error::BadSignature);
        }
        // 5. spend the nonce and move the funds.
        self.used_authorizations.set(&nonce_key, true);
        self.do_transfer(from, to, value);
        self.env().emit_event(AuthorizationUsed {
            from,
            to,
            amount: value,
            nonce,
        });
    }

    // ----- internal helpers (private — never exposed as entrypoints) -----

    fn do_transfer(&mut self, from: Address, to: Address, amount: U256) {
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        let from_bal = self.balances.get_or_default(&from);
        if from_bal < amount {
            self.env().revert(Error::InsufficientBalance);
        }
        self.balances.set(&from, from_bal - amount);
        let to_bal = self.balances.get_or_default(&to);
        let new_to = match to_bal.checked_add(amount) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        self.balances.set(&to, new_to);
        self.env().emit_event(Transfer { from, to, amount });
    }

    fn mint_to(&mut self, to: Address, amount: U256) {
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        let supply = self.total_supply.get_or_default();
        let new_supply = match supply.checked_add(amount) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        self.total_supply.set(new_supply);
        let bal = self.balances.get_or_default(&to);
        let new_bal = match bal.checked_add(amount) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        };
        self.balances.set(&to, new_bal);
        self.env().emit_event(Mint { to, amount });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::casper_types::bytesrepr::ToBytes;
    use odra::host::{Deployer, HostEnv};

    const SUPPLY: u64 = 1_000_000;
    const PAYER_BALANCE: u64 = 100_000;
    const WINDOW_START: u64 = 1_000;
    const WINDOW_END: u64 = 1_000_000;

    struct Fixture {
        env: HostEnv,
        token: X402TokenHostRef,
        payer: Address,
        relayer: Address,
        merchant: Address,
    }

    fn setup() -> Fixture {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let payer = env.get_account(1);
        let relayer = env.get_account(2);
        let merchant = env.get_account(3);
        env.set_caller(owner);
        let mut token = X402Token::deploy(
            &env,
            X402TokenInitArgs {
                name: "x402 USD".to_string(),
                symbol: "x402USD".to_string(),
                decimals: 6,
                initial_supply: U256::from(SUPPLY),
            },
        );
        // Provision the payer with a balance to authorize against.
        token.transfer(payer, U256::from(PAYER_BALANCE));
        Fixture {
            env,
            token,
            payer,
            relayer,
            merchant,
        }
    }

    /// Reconstruct the exact preimage the contract signs over, off-chain.
    fn auth_message(
        token: &Address,
        from: Address,
        to: Address,
        value: U256,
        valid_after: u64,
        valid_before: u64,
        nonce: &Bytes,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&token.to_bytes().unwrap());
        buf.extend_from_slice(&from.to_bytes().unwrap());
        buf.extend_from_slice(&to.to_bytes().unwrap());
        buf.extend_from_slice(&value.to_bytes().unwrap());
        buf.extend_from_slice(&valid_after.to_bytes().unwrap());
        buf.extend_from_slice(&valid_before.to_bytes().unwrap());
        buf.extend_from_slice(&nonce.to_bytes().unwrap());
        Bytes::from(buf)
    }

    /// Build a valid (publicKey, signature) for an authorization from `payer`.
    fn sign_auth(
        fx: &Fixture,
        to: Address,
        value: U256,
        valid_after: u64,
        valid_before: u64,
        nonce: &Bytes,
    ) -> (PublicKey, Bytes) {
        let token_addr = fx.token.address();
        let msg = auth_message(
            &token_addr,
            fx.payer,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
        );
        let sig = fx.env.sign_message(&msg, &fx.payer);
        let pk = fx.env.public_key(&fx.payer);
        (pk, sig)
    }

    #[test]
    fn relayer_settles_a_signed_authorization() {
        let mut fx = setup();
        fx.env.advance_block_time(WINDOW_START + 10);
        let nonce = Bytes::from(vec![9u8; 32]);
        let value = U256::from(25_000u64);
        let (pk, sig) = sign_auth(&fx, fx.merchant, value, WINDOW_START, WINDOW_END, &nonce);

        // A third party (the relayer) submits it; the payer spends no gas.
        fx.env.set_caller(fx.relayer);
        fx.token.transfer_with_authorization(
            fx.payer,
            fx.merchant,
            value,
            WINDOW_START,
            WINDOW_END,
            nonce.clone(),
            pk,
            sig,
        );

        assert_eq!(fx.token.balance_of(fx.merchant), value);
        assert_eq!(
            fx.token.balance_of(fx.payer),
            U256::from(PAYER_BALANCE) - value
        );
        assert!(fx.token.authorization_used(fx.payer, nonce));
    }

    #[test]
    fn rejects_replayed_authorization() {
        let mut fx = setup();
        fx.env.advance_block_time(WINDOW_START + 10);
        let nonce = Bytes::from(vec![1u8; 32]);
        let value = U256::from(10_000u64);
        let (pk, sig) = sign_auth(&fx, fx.merchant, value, WINDOW_START, WINDOW_END, &nonce);
        fx.env.set_caller(fx.relayer);
        fx.token.transfer_with_authorization(
            fx.payer,
            fx.merchant,
            value,
            WINDOW_START,
            WINDOW_END,
            nonce.clone(),
            pk.clone(),
            sig.clone(),
        );
        let err = fx
            .token
            .try_transfer_with_authorization(
                fx.payer,
                fx.merchant,
                value,
                WINDOW_START,
                WINDOW_END,
                nonce,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::AuthorizationAlreadyUsed.into());
    }

    #[test]
    fn rejects_tampered_amount() {
        let mut fx = setup();
        fx.env.advance_block_time(WINDOW_START + 10);
        let nonce = Bytes::from(vec![2u8; 32]);
        let signed_value = U256::from(10_000u64);
        let (pk, sig) = sign_auth(
            &fx,
            fx.merchant,
            signed_value,
            WINDOW_START,
            WINDOW_END,
            &nonce,
        );
        // Submit a larger value than was signed → signature no longer matches.
        fx.env.set_caller(fx.relayer);
        let err = fx
            .token
            .try_transfer_with_authorization(
                fx.payer,
                fx.merchant,
                U256::from(20_000u64),
                WINDOW_START,
                WINDOW_END,
                nonce,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_expired_authorization() {
        let mut fx = setup();
        let nonce = Bytes::from(vec![3u8; 32]);
        let value = U256::from(10_000u64);
        let (pk, sig) = sign_auth(&fx, fx.merchant, value, WINDOW_START, WINDOW_END, &nonce);
        fx.env.advance_block_time(WINDOW_END + 1); // past valid_before
        fx.env.set_caller(fx.relayer);
        let err = fx
            .token
            .try_transfer_with_authorization(
                fx.payer,
                fx.merchant,
                value,
                WINDOW_START,
                WINDOW_END,
                nonce,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::AuthorizationExpired.into());
    }

    #[test]
    fn rejects_signer_account_mismatch() {
        let mut fx = setup();
        fx.env.advance_block_time(WINDOW_START + 10);
        let nonce = Bytes::from(vec![4u8; 32]);
        let value = U256::from(10_000u64);
        let (pk, sig) = sign_auth(&fx, fx.merchant, value, WINDOW_START, WINDOW_END, &nonce);
        // Claim the authorization is "from" the merchant while the key is the payer's.
        fx.env.set_caller(fx.relayer);
        let err = fx
            .token
            .try_transfer_with_authorization(
                fx.merchant,
                fx.merchant,
                value,
                WINDOW_START,
                WINDOW_END,
                nonce,
                pk,
                sig,
            )
            .unwrap_err();
        assert_eq!(err, Error::NotAuthorizedSigner.into());
    }
}
