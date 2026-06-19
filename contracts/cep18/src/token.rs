//! A minimal, audited CEP-18 fungible token — Casper's fungible-token standard.
//!
//! In Cadence this is the demo's settlement stablecoin (e.g. test USDC): the buy
//! asset the agent's swaps settle into. It is owner-mintable so a testnet operator
//! can provision balances; otherwise it is a standard transfer / approve /
//! transfer_from token. Amounts are `U256` base units at `decimals` precision.

use odra::casper_types::U256;
use odra::prelude::*;

/// Emitted when balance moves between holders via `transfer` / `transfer_from`.
/// Freshly minted units are reported separately via [`Mint`], not here.
#[odra::event]
pub struct Transfer {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
}

/// Emitted when an allowance is set.
#[odra::event]
pub struct Approval {
    pub owner: Address,
    pub spender: Address,
    pub amount: U256,
}

/// Emitted when new units are minted into existence.
#[odra::event]
pub struct Mint {
    pub to: Address,
    pub amount: U256,
}

#[odra::odra_error]
pub enum Error {
    /// Transfer amount exceeds the holder's balance.
    InsufficientBalance = 1,
    /// `transfer_from` amount exceeds the caller's allowance.
    InsufficientAllowance = 2,
    /// Caller is not the token owner (the mint authority).
    NotOwner = 3,
    /// A zero amount was supplied where a positive value is required.
    ZeroAmount = 4,
    /// Arithmetic overflow in supply or balance.
    Overflow = 5,
}

/// A CEP-18 fungible token.
#[odra::module(events = [Transfer, Approval, Mint], errors = Error)]
pub struct Cep18 {
    name: Var<String>,
    symbol: Var<String>,
    decimals: Var<u8>,
    total_supply: Var<U256>,
    owner: Var<Address>,
    balances: Mapping<Address, U256>,
    allowances: Mapping<(Address, Address), U256>,
}

#[odra::module]
impl Cep18 {
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
    use odra::host::Deployer;

    const SUPPLY: u64 = 1_000_000;

    fn setup() -> (odra::host::HostEnv, Cep18HostRef) {
        let env = odra_test::env();
        let token = Cep18::deploy(
            &env,
            Cep18InitArgs {
                name: "Test USD Coin".to_string(),
                symbol: "tUSDC".to_string(),
                decimals: 6,
                initial_supply: U256::from(SUPPLY),
            },
        );
        (env, token)
    }

    #[test]
    fn init_mints_supply_to_deployer() {
        let (env, token) = setup();
        let owner = env.get_account(0);
        assert_eq!(token.total_supply(), U256::from(SUPPLY));
        assert_eq!(token.balance_of(owner), U256::from(SUPPLY));
        assert_eq!(token.symbol(), "tUSDC");
        assert_eq!(token.decimals(), 6);
    }

    #[test]
    fn transfers_move_balance() {
        let (env, mut token) = setup();
        let owner = env.get_account(0);
        let bob = env.get_account(1);
        token.transfer(bob, U256::from(400_000u64));
        assert_eq!(token.balance_of(bob), U256::from(400_000u64));
        assert_eq!(token.balance_of(owner), U256::from(600_000u64));
    }

    #[test]
    fn rejects_transfer_over_balance() {
        let (env, mut token) = setup();
        let bob = env.get_account(1);
        env.set_caller(bob); // bob holds nothing
        let err = token
            .try_transfer(env.get_account(0), U256::from(1u64))
            .unwrap_err();
        assert_eq!(err, Error::InsufficientBalance.into());
    }

    #[test]
    fn approve_and_transfer_from() {
        let (env, mut token) = setup();
        let owner = env.get_account(0);
        let spender = env.get_account(1);
        let dest = env.get_account(2);
        token.approve(spender, U256::from(250_000u64));
        assert_eq!(token.allowance(owner, spender), U256::from(250_000u64));

        env.set_caller(spender);
        token.transfer_from(owner, dest, U256::from(200_000u64));
        assert_eq!(token.balance_of(dest), U256::from(200_000u64));
        assert_eq!(token.allowance(owner, spender), U256::from(50_000u64));
    }

    #[test]
    fn rejects_transfer_from_over_allowance() {
        let (env, mut token) = setup();
        let owner = env.get_account(0);
        let spender = env.get_account(1);
        token.approve(spender, U256::from(100u64));
        env.set_caller(spender);
        let err = token
            .try_transfer_from(owner, spender, U256::from(101u64))
            .unwrap_err();
        assert_eq!(err, Error::InsufficientAllowance.into());
    }

    #[test]
    fn owner_can_mint_others_cannot() {
        let (env, mut token) = setup();
        let bob = env.get_account(1);
        token.mint(bob, U256::from(500u64)); // caller is owner (account 0)
        assert_eq!(token.balance_of(bob), U256::from(500u64));
        assert_eq!(token.total_supply(), U256::from(SUPPLY + 500));

        env.set_caller(bob);
        let err = token.try_mint(bob, U256::from(1u64)).unwrap_err();
        assert_eq!(err, Error::NotOwner.into());
    }
}
