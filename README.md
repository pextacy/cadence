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

## Live on Casper testnet

**App:** https://cadence-two-tawny.vercel.app — the dashboard reconstructs state
live from on-chain events (CSPR.cloud streaming). It is a static frontend (Vercel)
talking to a small proxy backend (Render) that injects credentials server-side; no
keys reach the browser.

How to test it, step by step:

1. Open the app and go to **Deployments** — the vault and swap-adapter packages
   are shown with explorer links (network: `casper-test`).
2. Open **Activity** — the vault's real testnet deploys are listed newest-first,
   fetched from chain.
3. Open **Execution** / the Cadence Stave — reconstructed from the vault's fill
   events; **AI Planner** shows the live Gemini proposal for the next slice.
4. Every hash links to [testnet.cspr.live](https://testnet.cspr.live).

Deployed contracts (contract **package** hashes, verifiable on-chain):

| Contract | Package hash |
|---|---|
| Execution Vault | `5af977b35dadd74eb4a14bc8b8edd3dd7fbba0a0e115ca4c012b5a2fbc90a014` |
| Cep18 Swap Adapter (venue) | `6c2bce9b90acb75238b640758b99904b6ff1fc243e765722397e045ac76b8dcb` |

Sample testnet transactions:

| Transaction | Deploy hash |
|---|---|
| Vault install (mandate signature verified on-chain) | [`692a3c1f…d3d768`](https://testnet.cspr.live/deploy/692a3c1f4d6b17c2d5a77d79b01e7961dc40e53fe433064b078dae5397d3d768) |
| Vault funded (100 CSPR) | [`3d778237…a52c49`](https://testnet.cspr.live/deploy/3d77823749fc3773cb6862da308426488733c19ef4ecea6ddfc2676f06a52c49) |
| `execute_slice` → atomic swap → fill | [`d3902a11…8f3594`](https://testnet.cspr.live/deploy/d3902a11c6503da231fdacd9a073493dfabe1599c3f90736c29c5c98fb8f3594) |

To run the full flow yourself against a fresh vault, see
[Running the demo (testnet)](#running-the-demo-testnet).

---

## Architecture

```
        sign mandate (EIP-712, gasless)        ┌──────────────────────┐
Treasurer ───────────────────────────────────► │  Mandate (typed data) │
   │ fund                                       └──────────┬───────────┘
   ▼                                                       │ digest + sig
┌─────────────────────────┐   execute_slice() within   ┌───┴────────────────┐
│  Execution Vault (Odra)  │◄───────────────────────────│   Cadence Agent     │
│  - custody (sell asset)  │   accept / REVERT          │  - Planner (Gemini) │
│  - mandate + limits      │───────────────────────────►│  - Executor (det.)  │
│  - fills + attestations  │        emits events        └───────┬────────────┘
└────────────┬─────────────┘                                    │ quotes / swaps
             │ stream (CSPR.cloud)            premium data (x402)│
             ▼                                                   ▼
       Live Dashboard                                  CSPR.trade MCP
```

- **`/contracts`** — a Cargo workspace of Odra (Rust → Casper WASM) contracts, one
  crate each: the **Execution Vault** (`vault/`, custodies the sell asset and
  re-validates every guardrail in `execute_slice`), a **CEP-18 token** (`cep18/`,
  the demo settlement stablecoin), and an **x402-payable token** (`x402-token/`,
  CEP-18 plus a gasless `transfer_with_authorization`). See
  [`contracts/README.md`](contracts/README.md).
- **`/mandate`** — shared TypeScript package: mandate schema, EIP-712 typed-data
  hashing, signing and verification (`@casper-ecosystem/casper-eip-712`).
- **`/agent`** — the agent service: an LLM **planner** (Google Gemini) that proposes the
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
# Contracts — runs every contract's tests (vault + CEP-18 + x402 token).
# Build a deployable wasm per crate with `./build-wasm.sh` (see contracts/README.md).
cd contracts && cargo test && cd ..

# TypeScript — mandate, agent and dashboard unit tests + builds
npm run build
npm test
```

## Running the demo (testnet)

The testnet venue is the self-contained on-chain `Cep18SwapAdapter` — an atomic
fixed-price pool we deploy ourselves, so the full vault → swap → settlement path
runs end to end with no external DEX. (The CSPR.trade MCP route the agent loop uses
for live quotes is mainnet-only; see [Mainnet route](#mainnet-route-csprtrade-live-quotes).)

```bash
# 1. Sign a mandate (gasless, offline). Writes mandate.signed.json.
npm run sign-mandate:testnet -w @cadence/scripts

# 2. Deploy the Execution Vault with the mandate's limits as constructor args.
#    Read the installed contract/package hash from the deploy and set
#    VAULT_CONTRACT_HASH / VAULT_PACKAGE_HASH in .env. (The mandate is signed
#    offline first and does not need the vault to exist — its EIP-712 domain is
#    chain-scoped, not bound to the package hash.)
npm run deploy:testnet -w @cadence/scripts

# 3. Deploy the on-chain swap adapter (the venue). Read its contract hash from the
#    deploy's named keys and set VENUE_ADDRESSES in .env.
npm run deploy-adapter:testnet -w @cadence/scripts

# 4. Fund the vault, point it at the adapter, set the pool price + reserve, then
#    have the agent release a real atomic slice: vault → adapter swap → treasury
#    paid, every step finalised on testnet with an explorer link.
npm run fund:testnet -w @cadence/scripts
npm run enable-and-slice:testnet -w @cadence/scripts

# 5. Watch it live.
npm run dev -w @cadence/dashboard
```

What the demo shows: the vault releases a slice, the adapter atomically swaps it and
pays the treasury, every transaction links to the testnet explorer, the fill is
recorded on-chain, the dashboard shows slippage saved versus a naive single sell,
and an attempted out-of-bounds trade is visibly **blocked by the contract**.

### Mainnet route (CSPR.trade live quotes)

On mainnet the agent loop (`npm run demo:mainnet`) routes each slice through the
CSPR.trade MCP instead of the local adapter: it fetches a fresh quote per
allowlisted venue, picks best execution, signs the returned swap locally
(non-custodial) and submits it. The public CSPR.trade MCP is mainnet-only, so this
route is exercised on mainnet rather than testnet.

## What is verified locally vs. on testnet

- **Verified by `cargo test`** — the vault: all six guardrails revert as required
  (over-cap, past-deadline, over-slippage, wrong venue, wrong caller, price band),
  the happy path executes/records/settles, settlement returns remaining funds, and
  `init` is one-shot; the CEP-18 token: mint/transfer/approve/transfer_from; the
  x402 token: a relayer-submitted signed authorization settles while tampered,
  replayed, expired and wrong-signer authorizations revert; plus the swap adapter,
  vault-factory, vault-registry, treasury-multisig, guardian, price-oracle,
  fee-module and access-control crates. (269 tests.)
- **Verified by `npm test`** — EIP-712 sign/verify and tamper-rejection (mandate),
  guardrail parity with the contract, slippage/price/min-out math, the planner
  output schema, the x402 payment-payload construction + signature round-trip, and
  the dashboard event reducer + metrics. (201 tests across packages.)
- **Requires a configured testnet + keys** — deploying the vault and the on-chain
  swap adapter, funding the vault, and the agent releasing a real atomic slice
  through the adapter (vault → swap → settlement), plus CSPR.cloud streaming for the
  dashboard. These run against the real endpoints; the only blanks are the `.env`
  values. Live quotes/swaps via the CSPR.trade MCP and x402 premium-data calls run
  on **mainnet** (the public CSPR.trade MCP is mainnet-only).

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
