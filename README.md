# Cadence — an Agentic OTC Execution Desk on Casper

Cadence executes large treasury orders on-chain without moving the market. A
treasurer signs **one mandate** ("sell 2,000,000 CSPR for USDC over 3 days, never
worse than 1% slippage"). An autonomous agent then slices and executes that order
over time, while an on-chain **Execution Vault** enforces hard limits the agent
**cannot** exceed.

One sentence to keep in mind: **the contract is the source of truth; the agent
only proposes and submits trades within the signed limits.**

See [`docs/prd.md`](docs/prd.md) for product context and [`docs/docs.md`](docs/docs.md)
for the technical spec. Contributor rules live in [`docs/claude.md`](docs/claude.md).

---

## Architecture

```
        sign mandate (EIP-712, gasless)        ┌──────────────────────┐
Treasurer ───────────────────────────────────► │  Mandate (typed data) │
   │ fund                                       └──────────┬───────────┘
   ▼                                                       │ digest + sig
┌─────────────────────────┐   execute_slice() within   ┌───┴────────────────┐
│  Execution Vault (Odra)  │◄───────────────────────────│   Cadence Agent     │
│  - custody (sell asset)  │   accept / REVERT          │  - Planner (Claude) │
│  - mandate + limits      │───────────────────────────►│  - Executor (det.)  │
│  - fills + attestations  │        emits events        └───────┬────────────┘
└────────────┬─────────────┘                                    │ quotes / swaps
             │ stream (CSPR.cloud)            premium data (x402)│
             ▼                                                   ▼
       Live Dashboard                                  CSPR.trade MCP
```

- **`/contracts`** — the Execution Vault (Rust + Odra). Custodies the sell asset,
  stores the mandate digest + decoded limits, and exposes a single constrained
  `execute_slice` entrypoint that re-validates every guardrail on-chain.
- **`/mandate`** — shared TypeScript package: mandate schema, EIP-712 typed-data
  hashing, signing and verification (`@casper-ecosystem/casper-eip-712`).
- **`/agent`** — the agent service: an LLM **planner** (Claude) that proposes the
  next slice, and a **deterministic executor** that validates every proposal
  against the mandate, submits `execute_slice`, performs the swap, records the
  fill and writes the attestation. Plus real client adapters for CSPR.trade MCP,
  the Casper MCP, the x402 facilitator and CSPR.cloud.
- **`/dashboard`** — a routed React app: Overview, then the mandate lifecycle
  (01 Mandate · 02 Execution · 03 Report). Its signature is the **Cadence Stave** —
  two aligned tracks (volume cadence + time cadence) that read together to answer
  "is the agent on tempo?". Flat, solid styling (Space Grotesk · IBM Plex Sans ·
  IBM Plex Mono); no gradients; real chain state only.
- **`/scripts`** — sign a mandate, deploy the vault, fund it, and run the demo.

## The guardrail principle

Every limit from the signed mandate is enforced **on-chain** in `execute_slice`:
spend cap, deadline, per-slice slippage, price band, venue allowlist and caller
authority. If any check fails the call **reverts**. The deterministic executor
pre-checks the same limits (in [`agent/src/executor/guardrails.ts`](agent/src/executor/guardrails.ts))
to avoid wasting a transaction, but the contract is the authority and validates
again regardless of what is submitted. An LLM hallucination is physically
incapable of breaching a limit.

## Setup

Prerequisites: Rust + [Odra](https://odra.dev) (`cargo install cargo-odra`),
Node 18+, and a Casper Testnet account with test CSPR.

```bash
npm install                 # installs all workspaces
cp .env.example .env        # fill in node RPC, API keys, agent/treasury keys
```

**Network.** Set `CASPER_NETWORK=testnet` or `mainnet` (and `VITE_CASPER_NETWORK`
for the dashboard) to switch everything — chain name, node RPC, CSPR.cloud
REST/streaming and the explorer all come from built-in presets. Any single
endpoint can still be overridden by its explicit variable. Defaults to testnet.

Build everything and run the test suites:

```bash
# Contracts — compiles the WASM and runs the guardrail tests
cd contracts && cargo odra test && cargo odra build && cd ..

# TypeScript — mandate, agent and dashboard unit tests + builds
npm run build
npm test
```

## Running the demo

```bash
# 1. Sign a mandate (gasless, offline). Writes mandate.signed.json.
npm run sign-mandate -w @cadence/scripts

# 2. Deploy the Execution Vault with the mandate's limits as constructor args.
#    Reads the agent identity from AGENT_ACCOUNT_HASH; prints the deploy hash.
npm run deploy:testnet -w @cadence/scripts
#    Read the installed contract hash from the deploy and set VAULT_CONTRACT_HASH
#    in .env. (The mandate is signed offline first and does not need the vault to
#    exist — its EIP-712 domain is chain-scoped, not bound to the package hash.)

# 3. Fund the vault and run the agent end to end on the configured pair.
npm run demo -w @cadence/scripts

# 4. Watch it live.
npm run dev -w @cadence/dashboard
```

What the demo shows: the agent fills the order over time, every swap links to the
testnet explorer, an x402 payment proof is logged, the dashboard shows slippage
saved versus a naive single sell, and an attempted out-of-bounds trade is visibly
**blocked by the contract**.

## What is verified locally vs. on testnet

- **Verified by `cargo odra test`** — all six guardrails revert as required
  (over-cap, past-deadline, over-slippage, wrong venue, wrong caller, price band),
  the happy path executes/records/settles, settlement returns remaining funds, and
  `init` is one-shot. (10 tests.)
- **Verified by `npm test`** — EIP-712 sign/verify and tamper-rejection (mandate),
  guardrail parity with the contract, slippage/price/min-out math, the planner
  output schema, the x402 payment-payload construction + signature round-trip, and
  the dashboard event reducer + metrics. (43 tests across packages.)
- **Requires a configured testnet + keys** — deploying the vault, funding it, live
  swaps via CSPR.trade, x402 premium-data calls, and CSPR.cloud streaming. These
  run against the real endpoints; the only blanks are the `.env` values.

## Design notes & trust boundary

- **Mandate authorisation.** The mandate is signed off-chain with an EIP-712
  typed-data signature (gasless, human-readable). The treasury is the
  Casper-authenticated caller of `init`, so on-chain authority comes from the
  sender; the EIP-712 digest + signature are bound on-chain (stored and emitted)
  so anyone can independently re-derive the digest from the public mandate and
  verify the signature. The EIP-712 domain is **chain-scoped**, not bound to the
  vault package (which does not exist when the mandate is signed). The
  mandate↔vault binding runs the other way: the vault stores the signed digest on
  `init`, committing itself to exactly one mandate, and the per-mandate `nonce`
  prevents replay. The mandate's limits are immutable for the life of the vault
  (`init` is a one-shot constructor).
- **Funding.** `fund` is an Odra `#[odra(payable)]` entrypoint; the attached CSPR
  is conveyed via Odra's payable calling convention (the `amount` runtime arg).
- **State reconstruction.** The dashboard reconstructs authoritative state purely
  from the vault's on-chain events via CSPR.cloud streaming — there is no mock
  data layer. When unconfigured or disconnected it shows honest loading/empty/
  error states rather than sample numbers.

## License

Prototype built for the Casper Agentic Buildathon 2026.
