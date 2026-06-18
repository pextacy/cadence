//! Execution Vault — the on-chain source of truth for a Cadence mandate.
//!
//! The vault custodies the sell asset (native CSPR for the demo pair), stores the
//! mandate digest and decoded limits, and exposes a single constrained spend
//! entrypoint (`execute_slice`) that the agent identity may call. Every guardrail
//! from the signed mandate is enforced here, on-chain: spend cap, deadline,
//! per-slice slippage, price band, venue allowlist and caller authority. If any
//! check fails the call reverts — this is the guardrail the agent cannot bypass.
//!
//! ## Two-signature authorization model
//!
//! The mandate is authored off-chain with an EIP-712 typed-data signature
//! (gasless, human-readable). That digest is Ethereum-flavoured (keccak256 +
//! secp256k1 *recover*) and a Casper contract **cannot** reproduce it: Odra
//! exposes `env().hash()` (**blake2b**, not keccak256) and
//! `env().verify_signature(message, signature, public_key)` (verify against a
//! *supplied* public key, not ecrecover). See the x402-token doc comment
//! (`contracts/x402-token/src/token.rs`).
//!
//! So the vault keeps the EIP-712 digest as the human-readable treasurer artifact
//! (stored + emitted for off-chain re-derivation) AND verifies a **Casper-native**
//! authorization on-chain. The treasury signs the canonical [`mandate_message`]
//! preimage — a domain tag, every enforced limit, and a unique nonce — with their
//! Casper key. At `init` the vault verifies that the supplied `PublicKey` hashes to
//! the `treasury` caller and that the signature verifies against the reconstructed
//! preimage; otherwise it reverts. This binds the on-chain limits provably to the
//! treasury's signature, closing the gap where the EIP-712 digest was stored but
//! never cryptographically checked. The pattern mirrors the x402-token
//! `transfer_with_authorization` flow (token.rs:176-247).
//!
//! ### Why the preimage binds the *nonce*, not `self_address`
//!
//! x402-token can prefix its preimage with `self_address()` because its
//! authorizations are signed **after** the token contract exists. A Cadence
//! mandate is signed **before** the vault is deployed (the vault's `init` *consumes*
//! the signature), so the vault address is unknowable at signing time — the same
//! chicken-and-egg the EIP-712 domain documents (`mandate/src/schema.ts`). Cross-
//! vault replay is therefore prevented by a unique per-mandate `nonce` baked into
//! the signed preimage (every mandate carries a distinct nonce), exactly as the
//! off-chain EIP-712 path already relies on. The domain tag versions the scheme.

use odra::casper_types::bytesrepr::{Bytes, ToBytes};
use odra::casper_types::{PublicKey, U512};
use odra::prelude::*;

/// Fixed-point scale for prices expressed as buy-asset units per one sell-asset
/// unit. A price of 1.0 is `PRICE_SCALE`. Using an integer scale keeps the
/// on-chain price-band check free of floating point.
pub const PRICE_SCALE: u64 = 1_000_000_000;

/// Basis-points denominator (100% = 10_000 bps).
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Domain tag prefixed to every Casper-native mandate preimage. Binds the
/// authorization to the Cadence mandate scheme (versioned) so a signature can never
/// be replayed under a different scheme version.
pub const MANDATE_DOMAIN_TAG: &[u8] = b"Cadence-Mandate-v1";

/// Lifecycle of a vault instance.
#[odra::odra_type]
pub enum Status {
    /// Mandate stored, not yet funded.
    Funded,
    /// Funded and executing.
    Active,
    /// Circuit-breaker engaged; execution suspended.
    Paused,
    /// Order completed (cap reached) and settled.
    Completed,
    /// Window closed before completion and settled.
    Expired,
    /// Emergency drain executed by the treasury while paused; terminal.
    Halted,
}

/// Emitted once when the mandate is stored.
#[odra::event]
pub struct MandateInitialised {
    pub treasury: Address,
    pub agent: Address,
    pub mandate_digest: Bytes,
    pub signature: Bytes,
    pub sell_asset: String,
    pub buy_asset: String,
    pub total_sell: U512,
    pub end_time_ms: u64,
    pub max_slippage_bps: u32,
}

/// Emitted by `init` AFTER a successful on-chain `verify_signature`, recording the
/// consumed mandate nonce and the blake2b hash of the canonical preimage that was
/// actually verified — a binding that was checked on-chain, not merely stored.
#[odra::event]
pub struct MandateVerified {
    pub treasury: Address,
    pub mandate_nonce: Bytes,
    pub preimage_hash: Bytes,
}

/// Emitted when the vault receives the sell asset and becomes Active.
#[odra::event]
pub struct VaultFunded {
    pub amount: U512,
    pub balance: U512,
}

/// Emitted for every accepted slice. `sell_amount` of the sell asset has been
/// released to the allowlisted venue for the swap; `min_out` is the floor the
/// agent committed to off-chain.
#[odra::event]
pub struct SliceExecuted {
    pub slice_id: u32,
    pub sell_amount: U512,
    pub quoted_out: U512,
    pub min_out: U512,
    pub venue: String,
    pub sold_so_far: U512,
}

/// Emitted when realised swap proceeds are recorded against a slice, linking the
/// on-chain swap deploy for the audit trail.
#[odra::event]
pub struct FillRecorded {
    pub slice_id: u32,
    pub bought_amount: U512,
    pub swap_deploy_hash: String,
    pub bought_so_far: U512,
}

/// Emitted for every decision attestation.
#[odra::event]
pub struct DecisionAttested {
    pub slice_id: u32,
    pub reason: String,
}

/// Emitted on pause / resume.
#[odra::event]
pub struct StatusChanged {
    pub paused: bool,
}

/// Emitted when the treasury triggers the emergency drain. Records who triggered
/// it, how much was returned, and the progress at the time of the halt.
#[odra::event]
pub struct EmergencyWithdrawn {
    pub by: Address,
    pub returned_to_treasury: U512,
    pub sold_so_far: U512,
}

/// Emitted once on settlement with the final execution report.
#[odra::event]
pub struct Settled {
    pub completed: bool,
    pub sold_so_far: U512,
    pub bought_so_far: U512,
    pub slice_count: u32,
    pub returned_to_treasury: U512,
}

#[odra::odra_error]
pub enum Error {
    /// Caller is not the treasury for this action.
    NotTreasury = 2,
    /// Caller is not the authorised agent identity.
    NotAgent = 3,
    /// Action requires `Active` status.
    NotActive = 4,
    /// Vault must be `Funded` (and not yet active) to be funded.
    NotFunded = 5,
    /// Execution window has closed (`now > end_time`).
    DeadlinePassed = 6,
    /// Slice would push cumulative sold size over the mandate cap.
    SpendCapExceeded = 7,
    /// Implied slippage (quoted_out vs min_out) exceeds the mandate cap.
    SlippageTooHigh = 8,
    /// Quoted price is outside the mandate's `[floor, ceiling]` band.
    PriceOutOfBand = 9,
    /// Venue is not on the mandate allowlist.
    VenueNotAllowed = 10,
    /// A zero amount was supplied where a positive value is required.
    ZeroAmount = 11,
    /// `min_out` exceeds `quoted_out`, which is nonsensical.
    MinOutAboveQuote = 12,
    /// Mandate digest must be exactly 32 bytes.
    BadDigestLength = 13,
    /// Settlement is only allowed after the deadline or on completion.
    CannotSettleYet = 14,
    /// Referenced slice id does not exist.
    UnknownSlice = 15,
    /// Funding amount did not match the mandate total.
    FundingMismatch = 16,
    /// The venue list and venue-address list had mismatched lengths at init.
    VenueConfigMismatch = 17,
    /// A fill was already recorded for this slice.
    SliceAlreadyFilled = 18,
    /// The supplied public key does not hash to the `treasury` account.
    NotAuthorizedSigner = 19,
    /// The Casper-native mandate signature does not verify against the preimage.
    BadSignature = 20,
    /// Failed to serialize the mandate preimage.
    SerializationError = 21,
    /// Emergency withdrawal requires the vault to be `Paused`.
    NotPaused = 22,
    /// Arithmetic overflow in a guardrail computation.
    Overflow = 23,
}

/// The Execution Vault.
#[odra::module(
    events = [
        MandateInitialised,
        MandateVerified,
        VaultFunded,
        SliceExecuted,
        FillRecorded,
        DecisionAttested,
        StatusChanged,
        EmergencyWithdrawn,
        Settled
    ],
    errors = Error
)]
pub struct ExecutionVault {
    // Identities
    treasury: Var<Address>,
    agent: Var<Address>,
    /// The treasury's Casper public key, captured at `init` after verification, so
    /// the Casper-native mandate signature can be independently re-checked on-chain.
    treasury_public_key: Var<PublicKey>,

    // Mandate binding
    mandate_digest: Var<Bytes>,
    signature: Var<Bytes>,
    /// The Casper-native mandate signature verified on-chain at `init`.
    casper_signature: Var<Bytes>,
    /// The consumed mandate nonce (part of the verified preimage).
    mandate_nonce: Var<Bytes>,

    // Decoded limits
    sell_asset: Var<String>,
    buy_asset: Var<String>,
    total_sell: Var<U512>,
    end_time_ms: Var<u64>,
    max_slippage_bps: Var<u32>,
    price_floor: Var<U512>,   // 0 == unset
    price_ceiling: Var<U512>, // 0 == unset
    venue_allowlist: Mapping<String, bool>,
    // The on-chain destination the sell asset is released to for each allowlisted
    // venue. Set once at `init` from the signed mandate; the agent cannot supply or
    // override it, so it can never redirect funds to an address it controls.
    venue_addr: Mapping<String, Address>,
    // Ordered, canonical copies of the venue id / address lists (Odra `Mapping` is
    // not enumerable). Stored so `verify_mandate` can rebuild the exact preimage.
    venue_ids: Var<Vec<String>>,
    venue_addr_list: Var<Vec<Address>>,

    // Progress
    sold_so_far: Var<U512>,
    bought_so_far: Var<U512>,
    slice_count: Var<u32>,
    // Per-slice min_out, keyed by slice id, so a fill can be reconciled.
    slice_min_out: Mapping<u32, U512>,
    slice_filled: Mapping<u32, bool>,

    status: Var<Status>,
}

#[odra::module]
impl ExecutionVault {
    /// Store the signed mandate and its decoded limits. Called once by the
    /// treasury. The caller becomes the treasury identity. `agent` is the
    /// account-abstraction identity later authorised to call `execute_slice`.
    ///
    /// The mandate's authority is established by a **Casper-native** signature:
    /// `treasury_public_key` must hash to the `init` caller, and `casper_signature`
    /// must verify against the canonical [`mandate_message`] preimage that encodes
    /// every enforced limit plus `mandate_nonce`. Either failure reverts, so the
    /// on-chain limits are provably the ones the treasury signed. The EIP-712
    /// `mandate_digest` is also stored/emitted for off-chain human re-derivation.
    ///
    /// Times are in **milliseconds** to match the Casper block time. Prices are
    /// fixed-point with [`PRICE_SCALE`]; pass `0` for an unset floor/ceiling.
    #[allow(clippy::too_many_arguments)]
    pub fn init(
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
    #[odra(payable)]
    pub fn fund(&mut self) {
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

    /// The single constrained spend entrypoint. Only the agent identity may call
    /// it. Every guardrail is re-validated here; any failure reverts. On success
    /// the slice's `sell_amount` is released to the allowlisted `venue_address`
    /// for the swap and the slice is recorded.
    ///
    /// - `sell_amount`  size of this child order in the sell asset
    /// - `quoted_out`   the venue quote's expected output for `sell_amount`
    /// - `min_out`      the agent's committed minimum acceptable output
    /// - `venue`        venue identifier (must be on the allowlist); the on-chain
    ///                  destination is resolved from the mandate, not the caller
    pub fn execute_slice(
        &mut self,
        sell_amount: U512,
        quoted_out: U512,
        min_out: U512,
        venue: String,
    ) -> u32 {
        self.assert_agent();

        // 1. status == Active
        if self.read_status() != Status::Active {
            self.env().revert(Error::NotActive);
        }
        // 2. now <= end_time
        if self.env().get_block_time() > self.end_time_ms.get_or_default() {
            self.env().revert(Error::DeadlinePassed);
        }
        // sanity on amounts
        if sell_amount.is_zero() || quoted_out.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        if min_out > quoted_out {
            self.env().revert(Error::MinOutAboveQuote);
        }
        // 3. sold_so_far + sell_amount <= total_sell (checked arithmetic).
        let new_sold = self.checked_add(self.sold_so_far.get_or_default(), sell_amount);
        if new_sold > self.total_sell.get_or_default() {
            self.env().revert(Error::SpendCapExceeded);
        }
        // 4. effective slippage (quote vs min_out) <= max_slippage_bps
        //    (quoted_out - min_out) * BPS_DENOMINATOR <= max_slippage_bps * quoted_out
        let slip_bps = self.max_slippage_bps.get_or_default() as u64;
        let lhs = self.checked_mul(quoted_out - min_out, U512::from(BPS_DENOMINATOR));
        let rhs = self.checked_mul(quoted_out, U512::from(slip_bps));
        if lhs > rhs {
            self.env().revert(Error::SlippageTooHigh);
        }
        // 5. quoted price within [price_floor, price_ceiling] (if set)
        //    price = quoted_out * PRICE_SCALE / sell_amount  (buy units per sell unit)
        let price = self.checked_mul(quoted_out, U512::from(PRICE_SCALE)) / sell_amount;
        let floor = self.price_floor.get_or_default();
        let ceiling = self.price_ceiling.get_or_default();
        if !floor.is_zero() && price < floor {
            self.env().revert(Error::PriceOutOfBand);
        }
        if !ceiling.is_zero() && price > ceiling {
            self.env().revert(Error::PriceOutOfBand);
        }
        // 6. venue ∈ allowlist — and resolve its mandate-bound destination. The
        //    caller never supplies the address, so it cannot redirect the funds.
        if !self.venue_allowlist.get_or_default(&venue) {
            self.env().revert(Error::VenueNotAllowed);
        }
        let venue_address = match self.venue_addr.get(&venue) {
            Some(addr) => addr,
            None => self.env().revert(Error::VenueNotAllowed),
        };

        // All guardrails passed. Checks-effects-interactions: record the slice
        // BEFORE releasing funds so the invariant audit is obvious and a future
        // re-entrant venue can never observe stale state.
        let slice_id = self.slice_count.get_or_default();
        self.sold_so_far.set(new_sold);
        self.slice_count.set(slice_id + 1);
        self.slice_min_out.set(&slice_id, min_out);
        self.slice_filled.set(&slice_id, false);

        self.env().emit_event(SliceExecuted {
            slice_id,
            sell_amount,
            quoted_out,
            min_out,
            venue,
            sold_so_far: new_sold,
        });

        self.env().transfer_tokens(&venue_address, &sell_amount);
        slice_id
    }

    /// Record realised swap proceeds for a slice and link the on-chain swap deploy
    /// hash. Called by the agent identity after the swap settles. Enforces that
    /// realised output is not below the slice's committed `min_out`.
    pub fn record_fill(&mut self, slice_id: u32, bought_amount: U512, swap_deploy_hash: String) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        // One fill per slice: a second call would double-count `bought_so_far`.
        if self.slice_filled.get_or_default(&slice_id) {
            self.env().revert(Error::SliceAlreadyFilled);
        }
        let min_out = self.slice_min_out.get_or_default(&slice_id);
        if bought_amount < min_out {
            self.env().revert(Error::SlippageTooHigh);
        }
        let bought = self.checked_add(self.bought_so_far.get_or_default(), bought_amount);
        self.bought_so_far.set(bought);
        self.slice_filled.set(&slice_id, true);
        self.env().emit_event(FillRecorded {
            slice_id,
            bought_amount,
            swap_deploy_hash,
            bought_so_far: bought,
        });
    }

    /// Record the agent's decision reasoning for a slice (audit trail).
    pub fn attest(&mut self, slice_id: u32, reason: String) {
        self.assert_agent();
        if slice_id >= self.slice_count.get_or_default() {
            self.env().revert(Error::UnknownSlice);
        }
        self.env().emit_event(DecisionAttested { slice_id, reason });
    }

    /// Circuit-breaker: pause execution. Agent or treasury may call.
    pub fn pause(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Active {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Paused);
        self.env().emit_event(StatusChanged { paused: true });
    }

    /// Resume after a pause. Agent or treasury may call.
    pub fn resume(&mut self) {
        self.assert_agent_or_treasury();
        if self.read_status() != Status::Paused {
            self.env().revert(Error::NotActive);
        }
        self.status.set(Status::Active);
        self.env().emit_event(StatusChanged { paused: false });
    }

    /// Emergency drain. **Treasury only**, and only while the vault is `Paused`.
    ///
    /// This is the incident kill-switch: when the circuit-breaker has paused the
    /// vault, the treasury may sweep the entire remaining balance back to itself and
    /// move the vault to the terminal `Halted` status, blocking any further
    /// execution. Distinct from `settle` (open-callable, only after the deadline or
    /// completion): `emergency_withdraw` lets the treasury recover funds the moment
    /// something goes wrong, without waiting for the window to close. Funds always
    /// return to the stored `treasury` — never to the agent, and never to a
    /// caller-supplied address.
    pub fn emergency_withdraw(&mut self) {
        self.assert_treasury();
        if self.read_status() != Status::Paused {
            self.env().revert(Error::NotPaused);
        }
        let treasury = self.treasury.get_or_revert_with(Error::NotTreasury);
        let remaining = self.env().self_balance();

        // Effects before interaction: mark terminal first, then transfer.
        self.status.set(Status::Halted);
        let sold = self.sold_so_far.get_or_default();
        self.env().emit_event(EmergencyWithdrawn {
            by: treasury,
            returned_to_treasury: remaining,
            sold_so_far: sold,
        });
        if !remaining.is_zero() {
            self.env().transfer_tokens(&treasury, &remaining);
        }
    }

    /// Settle the vault: return any remaining sell asset to the treasury and emit
    /// the final report. Callable by anyone once the order is complete (cap
    /// reached) or the window has closed. Distinguishes `Completed` vs `Expired`.
    ///
    /// Will not run once the vault is terminal (`Completed`/`Expired`/`Halted`); a
    /// halted vault has already returned its funds via `emergency_withdraw`.
    pub fn settle(&mut self) {
        let status = self.read_status();
        if status == Status::Completed || status == Status::Expired || status == Status::Halted {
            self.env().revert(Error::CannotSettleYet);
        }
        let sold = self.sold_so_far.get_or_default();
        let total = self.total_sell.get_or_default();
        let completed = sold >= total;
        let deadline_passed = self.env().get_block_time() > self.end_time_ms.get_or_default();
        if !completed && !deadline_passed {
            self.env().revert(Error::CannotSettleYet);
        }

        let remaining = self.env().self_balance();
        let treasury = self.treasury.get_or_revert_with(Error::NotTreasury);
        // Effects before interaction.
        self.status.set(if completed {
            Status::Completed
        } else {
            Status::Expired
        });
        self.env().emit_event(Settled {
            completed,
            sold_so_far: sold,
            bought_so_far: self.bought_so_far.get_or_default(),
            slice_count: self.slice_count.get_or_default(),
            returned_to_treasury: remaining,
        });
        if !remaining.is_zero() {
            self.env().transfer_tokens(&treasury, &remaining);
        }
    }

    // ----- read-only views (used by the dashboard / scripts) -----

    pub fn get_status(&self) -> Status {
        self.read_status()
    }
    pub fn get_treasury(&self) -> Address {
        self.treasury.get_or_revert_with(Error::NotTreasury)
    }
    pub fn get_agent(&self) -> Address {
        self.agent.get_or_revert_with(Error::NotAgent)
    }
    pub fn get_sell_asset(&self) -> String {
        self.sell_asset.get_or_default()
    }
    pub fn get_buy_asset(&self) -> String {
        self.buy_asset.get_or_default()
    }
    pub fn get_total_sell(&self) -> U512 {
        self.total_sell.get_or_default()
    }
    pub fn get_sold_so_far(&self) -> U512 {
        self.sold_so_far.get_or_default()
    }
    pub fn get_bought_so_far(&self) -> U512 {
        self.bought_so_far.get_or_default()
    }
    pub fn get_slice_count(&self) -> u32 {
        self.slice_count.get_or_default()
    }
    pub fn get_max_slippage_bps(&self) -> u32 {
        self.max_slippage_bps.get_or_default()
    }
    pub fn get_end_time_ms(&self) -> u64 {
        self.end_time_ms.get_or_default()
    }
    pub fn get_mandate_digest(&self) -> Bytes {
        self.mandate_digest.get_or_default()
    }
    pub fn get_mandate_nonce(&self) -> Bytes {
        self.mandate_nonce.get_or_default()
    }
    pub fn is_venue_allowed(&self, venue: String) -> bool {
        self.venue_allowlist.get_or_default(&venue)
    }

    /// Public view that re-runs the stored-limits → preimage → `verify_signature`
    /// check so anyone can independently confirm the on-chain limits match the
    /// treasury's Casper signature without trusting `init`-time emission. Returns
    /// `true` only if `treasury_public_key` hashes to the stored treasury AND the
    /// `signature` verifies against the canonical preimage of the stored limits.
    pub fn verify_mandate(&self, treasury_public_key: PublicKey, signature: Bytes) -> bool {
        let treasury = match self.treasury.get() {
            Some(t) => t,
            None => return false,
        };
        if Address::from(treasury_public_key.clone()) != treasury {
            return false;
        }
        let agent = match self.agent.get() {
            Some(a) => a,
            None => return false,
        };
        let preimage = self.mandate_message(
            agent,
            treasury,
            &self.sell_asset.get_or_default(),
            &self.buy_asset.get_or_default(),
            self.total_sell.get_or_default(),
            self.end_time_ms.get_or_default(),
            self.max_slippage_bps.get_or_default(),
            self.price_floor.get_or_default(),
            self.price_ceiling.get_or_default(),
            &self.venue_ids.get_or_default(),
            &self.venue_addr_list.get_or_default(),
            &self.mandate_nonce.get_or_default(),
        );
        self.env()
            .verify_signature(&preimage, &signature, &treasury_public_key)
    }

    // ----- internal helpers -----

    /// The canonical Casper-native mandate preimage that the treasury signs and the
    /// contract verifies. Mirrors the x402-token `authorization_message` discipline
    /// (token.rs:221-247): a domain tag plus every enforced mandate field in Casper
    /// `ToBytes` encoding. Cross-vault replay is prevented by the unique `nonce`
    /// (the vault address is unknowable at signing time — see the module docs).
    ///
    /// **Field order is frozen** and MUST be reproduced byte-for-byte off-chain
    /// (see `mandate/src/casperAuth.ts`):
    ///   domain_tag ‖ agent ‖ treasury ‖ sell_asset ‖ buy_asset ‖ total_sell ‖
    ///   end_time_ms ‖ max_slippage_bps ‖ price_floor ‖ price_ceiling ‖ venues ‖
    ///   venue_addresses ‖ nonce
    #[allow(clippy::too_many_arguments)]
    fn mandate_message(
        &self,
        agent: Address,
        treasury: Address,
        sell_asset: &str,
        buy_asset: &str,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: &[String],
        venue_addresses: &[Address],
        nonce: &Bytes,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        // Domain tag is a fixed prefix (not length-prefixed) — unambiguous because
        // the next field (`agent` Address) is itself self-describing in ToBytes.
        buf.extend_from_slice(MANDATE_DOMAIN_TAG);

        let parts: Vec<Result<Vec<u8>, _>> = vec![
            agent.to_bytes(),
            treasury.to_bytes(),
            sell_asset.to_string().to_bytes(),
            buy_asset.to_string().to_bytes(),
            total_sell.to_bytes(),
            end_time_ms.to_bytes(),
            max_slippage_bps.to_bytes(),
            price_floor.to_bytes(),
            price_ceiling.to_bytes(),
            venues.to_vec().to_bytes(),
            venue_addresses.to_vec().to_bytes(),
            nonce.to_bytes(),
        ];
        for part in parts {
            match part {
                Ok(bytes) => buf.extend_from_slice(&bytes),
                Err(_) => self.env().revert(Error::SerializationError),
            }
        }
        Bytes::from(buf)
    }

    fn read_status(&self) -> Status {
        self.status.get_or_revert_with(Error::NotFunded)
    }

    fn checked_add(&self, a: U512, b: U512) -> U512 {
        match a.checked_add(b) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        }
    }

    fn checked_mul(&self, a: U512, b: U512) -> U512 {
        match a.checked_mul(b) {
            Some(v) => v,
            None => self.env().revert(Error::Overflow),
        }
    }

    fn assert_treasury(&self) {
        if self.env().caller() != self.treasury.get_or_revert_with(Error::NotTreasury) {
            self.env().revert(Error::NotTreasury);
        }
    }

    fn assert_agent(&self) {
        if self.env().caller() != self.agent.get_or_revert_with(Error::NotAgent) {
            self.env().revert(Error::NotAgent);
        }
    }

    fn assert_agent_or_treasury(&self) {
        let caller = self.env().caller();
        let is_agent = caller == self.agent.get_or_revert_with(Error::NotAgent);
        let is_treasury = caller == self.treasury.get_or_revert_with(Error::NotTreasury);
        if !is_agent && !is_treasury {
            self.env().revert(Error::NotAgent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::{Deployer, HostEnv, HostRef};

    const TOTAL_SELL: u64 = 1_000_000;
    const END_TIME_MS: u64 = 1_000_000;
    const SLIPPAGE_BPS: u32 = 100; // 1%

    struct Fixture {
        env: HostEnv,
        contract: ExecutionVaultHostRef,
        treasury: Address,
        agent: Address,
        venue_addr: Address,
    }

    fn digest32() -> Bytes {
        Bytes::from(vec![7u8; 32])
    }

    fn nonce32() -> Bytes {
        Bytes::from(vec![5u8; 32])
    }

    fn venues() -> Vec<String> {
        vec!["cspr.trade".to_string()]
    }

    /// Reconstruct the exact canonical preimage the contract signs over, off-chain.
    /// Byte-for-byte identical to [`ExecutionVault::mandate_message`].
    #[allow(clippy::too_many_arguments)]
    fn mandate_message_offchain(
        agent: Address,
        treasury: Address,
        sell_asset: &str,
        buy_asset: &str,
        total_sell: U512,
        end_time_ms: u64,
        max_slippage_bps: u32,
        price_floor: U512,
        price_ceiling: U512,
        venues: &[String],
        venue_addresses: &[Address],
        nonce: &Bytes,
    ) -> Bytes {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(MANDATE_DOMAIN_TAG);
        buf.extend_from_slice(&agent.to_bytes().unwrap());
        buf.extend_from_slice(&treasury.to_bytes().unwrap());
        buf.extend_from_slice(&sell_asset.to_string().to_bytes().unwrap());
        buf.extend_from_slice(&buy_asset.to_string().to_bytes().unwrap());
        buf.extend_from_slice(&total_sell.to_bytes().unwrap());
        buf.extend_from_slice(&end_time_ms.to_bytes().unwrap());
        buf.extend_from_slice(&max_slippage_bps.to_bytes().unwrap());
        buf.extend_from_slice(&price_floor.to_bytes().unwrap());
        buf.extend_from_slice(&price_ceiling.to_bytes().unwrap());
        buf.extend_from_slice(&venues.to_vec().to_bytes().unwrap());
        buf.extend_from_slice(&venue_addresses.to_vec().to_bytes().unwrap());
        buf.extend_from_slice(&nonce.to_bytes().unwrap());
        Bytes::from(buf)
    }

    /// All-purpose deploy helper. The preimage is address-independent (bound by the
    /// nonce, not `self_address`), so the treasury can sign it before the vault
    /// exists — exactly the production flow. Parameters let the adversarial tests
    /// deliberately mis-sign or tamper the installed limits.
    struct DeployArgs {
        price_floor: U512,
        price_ceiling: U512,
        /// Account whose key signs the canonical preimage.
        signer: Address,
        /// Account whose public key is supplied to `init`.
        supplied_pk_account: Address,
        /// total_sell value baked into the SIGNED preimage.
        signed_total: U512,
        /// total_sell value actually INSTALLED via init (differs to simulate tamper).
        install_total: U512,
        /// If set, an explicit raw signature to use instead of signing the preimage
        /// (for the forged-bytes case).
        override_signature: Option<Bytes>,
    }

    fn try_deploy(env: &HostEnv, args: DeployArgs) -> Result<ExecutionVaultHostRef, OdraError> {
        let treasury = env.get_account(0);
        let agent = env.get_account(1);
        let venue_addr = env.get_account(2);
        env.set_caller(treasury);

        let preimage = mandate_message_offchain(
            agent,
            treasury,
            "CSPR",
            "USDC",
            args.signed_total,
            END_TIME_MS,
            SLIPPAGE_BPS,
            args.price_floor,
            args.price_ceiling,
            &venues(),
            &[venue_addr],
            &nonce32(),
        );
        let casper_signature = args
            .override_signature
            .unwrap_or_else(|| env.sign_message(&preimage, &args.signer));
        let supplied_pk = env.public_key(&args.supplied_pk_account);

        ExecutionVault::try_deploy(
            env,
            ExecutionVaultInitArgs {
                agent,
                mandate_digest: digest32(),
                signature: Bytes::from(vec![1u8; 65]),
                treasury_public_key: supplied_pk,
                casper_signature,
                mandate_nonce: nonce32(),
                sell_asset: "CSPR".to_string(),
                buy_asset: "USDC".to_string(),
                total_sell: args.install_total,
                end_time_ms: END_TIME_MS,
                max_slippage_bps: SLIPPAGE_BPS,
                price_floor: args.price_floor,
                price_ceiling: args.price_ceiling,
                venues: venues(),
                venue_addresses: vec![venue_addr],
            },
        )
    }

    fn deploy_with(price_floor: U512, price_ceiling: U512) -> Fixture {
        let env = odra_test::env();
        let treasury = env.get_account(0);
        let agent = env.get_account(1);
        let venue_addr = env.get_account(2);
        let contract = try_deploy(
            &env,
            DeployArgs {
                price_floor,
                price_ceiling,
                signer: treasury,
                supplied_pk_account: treasury,
                signed_total: U512::from(TOTAL_SELL),
                install_total: U512::from(TOTAL_SELL),
                override_signature: None,
            },
        )
        .expect("happy-path deploy must verify");
        Fixture {
            env,
            contract,
            treasury,
            agent,
            venue_addr,
        }
    }

    fn fund(fx: &mut Fixture) {
        fx.env.set_caller(fx.treasury);
        fx.contract.with_tokens(U512::from(TOTAL_SELL)).fund();
    }

    /// A slice priced at 2.0 with exactly 1% slippage — passes all guardrails.
    fn ok_slice(fx: &mut Fixture) -> u32 {
        fx.env.set_caller(fx.agent);
        fx.contract.execute_slice(
            U512::from(100_000u64),
            U512::from(200_000u64),
            U512::from(198_000u64),
            "cspr.trade".to_string(),
        )
    }

    // ---------------------------------------------------------------------------
    // Existing behavioural coverage (preserved)
    // ---------------------------------------------------------------------------

    #[test]
    fn happy_path_executes_records_and_settles() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        assert_eq!(fx.contract.get_status(), Status::Funded);
        fund(&mut fx);
        assert_eq!(fx.contract.get_status(), Status::Active);

        let id = ok_slice(&mut fx);
        assert_eq!(id, 0);
        assert_eq!(fx.contract.get_sold_so_far(), U512::from(100_000u64));
        assert_eq!(fx.contract.get_slice_count(), 1);

        fx.env.set_caller(fx.agent);
        fx.contract
            .record_fill(0, U512::from(199_000u64), "deploy-hash-abc".to_string());
        assert_eq!(fx.contract.get_bought_so_far(), U512::from(199_000u64));
        fx.contract.attest(0, "TWAP slice 1 of 10".to_string());

        // Fill the rest of the cap so the order completes, then settle.
        for _ in 0..9 {
            ok_slice(&mut fx);
        }
        assert_eq!(fx.contract.get_sold_so_far(), U512::from(TOTAL_SELL));

        fx.contract.settle();
        assert_eq!(fx.contract.get_status(), Status::Completed);
    }

    #[test]
    fn rejects_non_agent_caller() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.treasury); // not the agent
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::NotAgent.into());
    }

    #[test]
    fn rejects_over_spend_cap() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(TOTAL_SELL + 1),
                U512::from(2_000_002u64),
                U512::from(1_980_001u64),
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::SpendCapExceeded.into());
    }

    #[test]
    fn rejects_past_deadline() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.advance_block_time(END_TIME_MS + 1);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::DeadlinePassed.into());
    }

    #[test]
    fn rejects_excessive_slippage() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        // min_out 197_000 of quote 200_000 => 1.5% > 1% cap.
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(197_000u64),
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::SlippageTooHigh.into());
    }

    #[test]
    fn rejects_non_allowlisted_venue() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "evil.dex".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::VenueNotAllowed.into());
    }

    #[test]
    fn rejects_price_outside_band() {
        // Band [1.5, 2.5]; a quote priced at 3.0 must revert.
        let mut fx = deploy_with(U512::from(1_500_000_000u64), U512::from(2_500_000_000u64));
        fund(&mut fx);
        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(300_000u64), // price 3.0
                U512::from(300_000u64), // zero slippage so only price check fails
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::PriceOutOfBand.into());
    }

    #[test]
    fn rejects_funding_mismatch() {
        let fx = deploy_with(U512::zero(), U512::zero());
        fx.env.set_caller(fx.treasury);
        let err = fx
            .contract
            .with_tokens(U512::from(TOTAL_SELL - 1))
            .try_fund()
            .unwrap_err();
        assert_eq!(err, Error::FundingMismatch.into());
    }

    #[test]
    fn init_is_one_shot() {
        // The mandate's limits are immutable for the life of the vault. Odra
        // enforces this at the framework level: `init` is a constructor and cannot
        // be re-invoked after install, so a second attempt must error.
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fx.env.set_caller(fx.treasury);
        let pk = fx.env.public_key(&fx.treasury);
        let result = fx.contract.try_init(
            fx.agent,
            digest32(),
            Bytes::from(vec![1u8; 65]),
            pk,
            Bytes::from(vec![1u8; 65]),
            nonce32(),
            "CSPR".to_string(),
            "USDC".to_string(),
            U512::from(TOTAL_SELL),
            END_TIME_MS,
            SLIPPAGE_BPS,
            U512::zero(),
            U512::zero(),
            venues(),
            vec![fx.venue_addr],
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_double_fill() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        ok_slice(&mut fx);
        fx.env.set_caller(fx.agent);
        fx.contract
            .record_fill(0, U512::from(199_000u64), "deploy-hash-1".to_string());
        let err = fx
            .contract
            .try_record_fill(0, U512::from(199_000u64), "deploy-hash-2".to_string())
            .unwrap_err();
        assert_eq!(err, Error::SliceAlreadyFilled.into());
    }

    #[test]
    fn settle_after_deadline_marks_expired_and_returns_funds() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        ok_slice(&mut fx); // sell 100_000 of 1_000_000
        let treasury_before = fx.env.balance_of(&fx.treasury);
        fx.env.advance_block_time(END_TIME_MS + 1);
        fx.contract.settle();
        assert_eq!(fx.contract.get_status(), Status::Expired);
        // Remaining 900_000 returned to treasury.
        let treasury_after = fx.env.balance_of(&fx.treasury);
        assert!(treasury_after > treasury_before);
    }

    // ---------------------------------------------------------------------------
    // (a) On-chain mandate signature verification — adversarial matrix
    // ---------------------------------------------------------------------------

    #[test]
    fn happy_path_signature_verifies_at_init() {
        // The deploy helper signs the canonical preimage with the treasury key and
        // supplies the treasury public key. If `init`'s on-chain verification did
        // not pass, deploy would revert and `deploy_with` would panic.
        let fx = deploy_with(U512::zero(), U512::zero());
        assert_eq!(fx.contract.get_status(), Status::Funded);
        // The stored nonce is the one bound into the verified preimage.
        assert_eq!(fx.contract.get_mandate_nonce(), nonce32());
    }

    #[test]
    fn verify_mandate_view_confirms_stored_limits() {
        let fx = deploy_with(U512::zero(), U512::zero());
        let pk = fx.env.public_key(&fx.treasury);
        // The treasury re-signs the SAME canonical preimage over the stored limits;
        // the view must accept it.
        let preimage = mandate_message_offchain(
            fx.agent,
            fx.treasury,
            "CSPR",
            "USDC",
            U512::from(TOTAL_SELL),
            END_TIME_MS,
            SLIPPAGE_BPS,
            U512::zero(),
            U512::zero(),
            &venues(),
            &[fx.venue_addr],
            &nonce32(),
        );
        let sig = fx.env.sign_message(&preimage, &fx.treasury);
        assert!(fx.contract.verify_mandate(pk, sig));
    }

    #[test]
    fn verify_mandate_view_rejects_wrong_key() {
        // A public key that does not hash to the stored treasury must be rejected by
        // the view even before signature verification.
        let fx = deploy_with(U512::zero(), U512::zero());
        let agent_pk = fx.env.public_key(&fx.agent);
        let preimage = mandate_message_offchain(
            fx.agent,
            fx.treasury,
            "CSPR",
            "USDC",
            U512::from(TOTAL_SELL),
            END_TIME_MS,
            SLIPPAGE_BPS,
            U512::zero(),
            U512::zero(),
            &venues(),
            &[fx.venue_addr],
            &nonce32(),
        );
        // Even a well-formed signature over the right preimage fails: wrong key.
        let sig = fx.env.sign_message(&preimage, &fx.agent);
        assert!(!fx.contract.verify_mandate(agent_pk, sig));
    }

    #[test]
    fn rejects_forged_signature_at_init() {
        // A well-formed signature over a DIFFERENT message (the treasury signing
        // unrelated bytes) does not verify against the mandate preimage → the
        // constructor reverts with BadSignature. Using a real signature keeps the
        // bytes deserializable so we exercise the verify path, not the decode path.
        let env = odra_test::env();
        let treasury = env.get_account(0);
        let unrelated = Bytes::from(b"not-the-mandate".to_vec());
        let forged = env.sign_message(&unrelated, &treasury);
        let result = try_deploy(
            &env,
            DeployArgs {
                price_floor: U512::zero(),
                price_ceiling: U512::zero(),
                signer: treasury,
                supplied_pk_account: treasury,
                signed_total: U512::from(TOTAL_SELL),
                install_total: U512::from(TOTAL_SELL),
                override_signature: Some(forged),
            },
        );
        let err = result.map(|_| ()).unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_wrong_signer_key_hash_mismatch() {
        // The supplied public key belongs to the AGENT, not the treasury caller, so
        // `Address::from(pk) != caller` and init reverts NotAuthorizedSigner — even
        // though the signature is itself well-formed.
        let env = odra_test::env();
        let agent = env.get_account(1);
        let result = try_deploy(
            &env,
            DeployArgs {
                price_floor: U512::zero(),
                price_ceiling: U512::zero(),
                signer: agent,              // signs with the agent key
                supplied_pk_account: agent, // supplies the agent public key
                signed_total: U512::from(TOTAL_SELL),
                install_total: U512::from(TOTAL_SELL),
                override_signature: None,
            },
        );
        let err = result.map(|_| ()).unwrap_err();
        assert_eq!(err, Error::NotAuthorizedSigner.into());
    }

    #[test]
    fn rejects_wrong_signer_signed_by_attacker() {
        // The attacker signs the preimage with their OWN key but the treasury public
        // key is supplied (so the key-hash check passes) → verify_signature fails
        // because the signature was not produced by the treasury key.
        let env = odra_test::env();
        let attacker = env.get_account(3);
        let result = try_deploy(
            &env,
            DeployArgs {
                price_floor: U512::zero(),
                price_ceiling: U512::zero(),
                signer: attacker, // signs with an attacker key
                supplied_pk_account: env.get_account(0), // supplies treasury pk
                signed_total: U512::from(TOTAL_SELL),
                install_total: U512::from(TOTAL_SELL),
                override_signature: None,
            },
        );
        let err = result.map(|_| ()).unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_tampered_limits_signature_mismatch() {
        // The treasury signed a preimage with total_sell = TOTAL_SELL, but init is
        // invoked with a DIFFERENT total_sell. The reconstructed preimage no longer
        // matches what was signed → BadSignature. This is the core property: the
        // installed limits are provably the ones the treasury signed.
        let env = odra_test::env();
        let treasury = env.get_account(0);
        let result = try_deploy(
            &env,
            DeployArgs {
                price_floor: U512::zero(),
                price_ceiling: U512::zero(),
                signer: treasury,
                supplied_pk_account: treasury,
                signed_total: U512::from(TOTAL_SELL), // signed value
                install_total: U512::from(TOTAL_SELL * 2), // tampered installed value
                override_signature: None,
            },
        );
        let err = result.map(|_| ()).unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    #[test]
    fn rejects_replayed_signature_under_different_limits() {
        // Replay: take a perfectly valid signature for one mandate and try to reuse
        // it for a vault with different price-band limits (a different preimage).
        // The same signature can no longer verify → BadSignature, so a captured
        // authorization cannot be replayed onto a vault it was not signed for.
        let env = odra_test::env();
        let treasury = env.get_account(0);
        let venue_addr = env.get_account(2);
        // Sign the ORIGINAL preimage (no band).
        let original = mandate_message_offchain(
            env.get_account(1),
            treasury,
            "CSPR",
            "USDC",
            U512::from(TOTAL_SELL),
            END_TIME_MS,
            SLIPPAGE_BPS,
            U512::zero(),
            U512::zero(),
            &venues(),
            &[venue_addr],
            &nonce32(),
        );
        let captured_sig = env.sign_message(&original, &treasury);
        // Replay it onto a vault that installs a non-zero price band.
        let result = try_deploy(
            &env,
            DeployArgs {
                price_floor: U512::from(1_500_000_000u64),
                price_ceiling: U512::from(2_500_000_000u64),
                signer: treasury,
                supplied_pk_account: treasury,
                signed_total: U512::from(TOTAL_SELL),
                install_total: U512::from(TOTAL_SELL),
                override_signature: Some(captured_sig),
            },
        );
        let err = result.map(|_| ()).unwrap_err();
        assert_eq!(err, Error::BadSignature.into());
    }

    // ---------------------------------------------------------------------------
    // (b) emergency_withdraw — treasury-only, requires Paused
    // ---------------------------------------------------------------------------

    #[test]
    fn emergency_withdraw_drains_to_treasury_when_paused() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        ok_slice(&mut fx); // releases 100_000, leaves 900_000 in the vault

        // Treasury pauses, then drains.
        fx.env.set_caller(fx.treasury);
        fx.contract.pause();
        assert_eq!(fx.contract.get_status(), Status::Paused);

        let before = fx.env.balance_of(&fx.treasury);
        fx.env.set_caller(fx.treasury);
        fx.contract.emergency_withdraw();
        assert_eq!(fx.contract.get_status(), Status::Halted);
        let after = fx.env.balance_of(&fx.treasury);
        assert!(after > before, "remaining balance must return to treasury");
    }

    #[test]
    fn emergency_withdraw_rejects_non_treasury() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.treasury);
        fx.contract.pause();
        // Agent attempts the drain — must be rejected.
        fx.env.set_caller(fx.agent);
        let err = fx.contract.try_emergency_withdraw().unwrap_err();
        assert_eq!(err, Error::NotTreasury.into());
    }

    #[test]
    fn emergency_withdraw_requires_paused() {
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx); // Active, not Paused
        fx.env.set_caller(fx.treasury);
        let err = fx.contract.try_emergency_withdraw().unwrap_err();
        assert_eq!(err, Error::NotPaused.into());
    }

    #[test]
    fn settle_blocked_after_emergency_withdraw() {
        // Once halted, settle must not move funds again (terminal state).
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.treasury);
        fx.contract.pause();
        fx.env.set_caller(fx.treasury);
        fx.contract.emergency_withdraw();
        assert_eq!(fx.contract.get_status(), Status::Halted);

        fx.env.advance_block_time(END_TIME_MS + 1);
        let err = fx.contract.try_settle().unwrap_err();
        assert_eq!(err, Error::CannotSettleYet.into());
    }

    #[test]
    fn execute_slice_blocked_while_halted() {
        // After an emergency drain the agent cannot resume execution: status is
        // Halted (not Active), so execute_slice reverts NotActive.
        let mut fx = deploy_with(U512::zero(), U512::zero());
        fund(&mut fx);
        fx.env.set_caller(fx.treasury);
        fx.contract.pause();
        fx.env.set_caller(fx.treasury);
        fx.contract.emergency_withdraw();

        fx.env.set_caller(fx.agent);
        let err = fx
            .contract
            .try_execute_slice(
                U512::from(100_000u64),
                U512::from(200_000u64),
                U512::from(198_000u64),
                "cspr.trade".to_string(),
            )
            .unwrap_err();
        assert_eq!(err, Error::NotActive.into());
    }
}
