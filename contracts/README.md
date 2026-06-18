# Cadence contracts

A Cargo workspace of the Odra (Rust → Casper WASM) contracts behind Cadence. Each
contract is its own member crate so it builds and deploys independently.

| Crate | Module | Contract | Role |
|---|---|---|---|
| [`vault/`](vault) | `vault::ExecutionVault` | Execution Vault | Custodies the sell asset and enforces every mandate guardrail on-chain (spend cap, deadline, slippage, price band, venue, caller). The source of truth the agent cannot bypass. |
| [`cep18/`](cep18) | `token::Cep18` | CEP-18 token | The demo settlement stablecoin (e.g. test USDC) the swaps settle into. Standard transfer / approve / transfer_from, owner-mintable for testnet provisioning. |
| [`x402-token/`](x402-token) | `token::X402Token` | x402-payable token | A CEP-18 token plus `transfer_with_authorization` — the on-chain x402 settlement primitive: a payer signs an authorization off-chain and any relayer submits it, so the payer spends no gas. |

## x402 on-chain authorization — Casper-native, not EIP-712

The off-chain x402 reference (`agent/src/clients/x402.ts`) builds an Ethereum
EIP-712 `TransferAuthorization` (keccak256 + secp256k1 *recover*) for the external
CSPR.cloud facilitator. An Odra contract cannot reproduce that: the framework
exposes `env().hash()` (**blake2b**, not keccak256) and
`env().verify_signature(message, signature, public_key)` (verify against a
*supplied* key, not ecrecover). `X402Token::transfer_with_authorization`
therefore authorizes with Casper's native signature scheme — same x402 semantics
(gasless for the payer, replay-protected via a per-`(from, nonce)` record,
time-bounded by `valid_after`/`valid_before`), Casper-native crypto. Wiring the
TS client to settle through this contract (vs. the facilitator) would require
Casper-native signing and is left as a follow-up.

## Build & test

```bash
# Run every contract's guardrail/behaviour tests (Odra VM backend — no wasm needed).
cargo test

# Lint.
cargo clippy --all-targets

# Build a deployable wasm for one contract (writes <crate>/wasm/<Contract>.wasm).
cd vault       && cargo odra build -b casper && cd ..
cd cep18       && cargo odra build -b casper && cd ..
cd x402-token  && cargo odra build -b casper && cd ..
```

Tests verified by `cargo test` (21 total): the vault's ten guardrail/lifecycle
tests, the CEP-18 token's six (mint/transfer/approve/transfer_from), and the
x402 token's five (a relayer-submitted signed authorization settles; tampered,
replayed, expired, and wrong-signer authorizations all revert).
