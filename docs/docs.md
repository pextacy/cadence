# Cadence — Technical Documentation (DOCS.md)

Developer-facing documentation for **Cadence**, an agentic OTC execution desk on the Casper Network. For product context see `PRD.md`; for contributor rules see `CLAUDE.md`.

---

## 1. Overview

Cadence executes large treasury orders with minimal market impact. The treasurer authorises a single **mandate**; an autonomous agent then plans and executes the order over time, while an on-chain **Execution Vault** enforces the mandate's limits. The agent operates under its own on-chain identity (account abstraction) and has no power to act outside the signed constraints.

Design principle: **plan in the agent, enforce in the contract.** The LLM is a planner; a deterministic executor and the vault are the safety boundary.

## 2. System architecture

```
┌──────────────┐  1. sign mandate (EIP-712, gasless)   ┌──────────────────────┐
│  Treasurer   │──────────────────────────────────────►│  Mandate (typed data)│
│   (UI)       │  2. fund vault                         └──────────┬───────────┘
└──────┬───────┘                                                   │ hash + sig
       │ fund (sell asset)                                         ▼
       ▼                                              ┌─────────────────────────┐
┌─────────────────────────┐   on-chain enforce        │   Execution Vault (Odra) │
│   Cadence Agent       │──── execute_slice() ─────►│   - custody              │
│   ┌──────────┐           │◄─── accept / reject ──────│   - mandate + limits     │
│   │ Planner  │ (LLM)     │                           │   - fills + attestation  │
│   └────┬─────┘           │   read state (Casper MCP) │   - settle / report      │
│        │ schedule        │◄──────────────────────────└────────────┬────────────┘
│   ┌────▼─────┐           │                                        │ events
│   │ Executor │ (det.)    │  quotes / routes / swaps               ▼
│   └────┬─────┘           │  ┌────────────────────┐        ┌───────────────┐
│        │ ───────────────────► CSPR.trade MCP     │        │  Dashboard     │
│        │ premium data (x402)  └────────────────────┘        │ (CSPR.cloud    │
│        └────────────────────► x402 Facilitator              │  streaming)    │
└──────────────────────────┘                                  └───────────────┘
```

### Components
- **Mandate** — typed, human-readable order authorisation signed off-chain (EIP-712 style). Not a transaction; costs no gas.
- **Execution Vault (Odra/Rust)** — custodies the sell asset, stores the mandate hash and limits, verifies the signature, exposes a constrained execution entrypoint, records fills and attestations, and settles.
- **Planner (LLM)** — converts a mandate + market state into a slicing schedule (child-order sizes and timing). Stateless w.r.t. authority; produces proposals only.
- **Executor (deterministic)** — fetches quotes, validates proposals against limits, submits swaps, writes attestations, handles retries and pausing.
- **Clients** — thin adapters over CSPR.trade MCP, Casper MCP, x402, and CSPR.cloud.
- **Dashboard** — subscribes to vault events via CSPR.cloud streaming and renders progress + the slippage-saved metric.

## 3. The mandate

A mandate is the only thing the treasurer signs. Suggested schema:

```jsonc
{
  "version": 1,
  "treasury": "<treasury account / vault owner identity>",
  "sellAsset": "CSPR",
  "buyAsset": "<stablecoin token id>",
  "totalSellAmount": "2000000000000000",   // motes, fixed-point
  "startTime": 1751328000,                  // unix seconds
  "endTime": 1751587200,                    // execution window close
  "maxSlippageBps": 100,                    // 1.00%
  "priceFloor": "0.0",                      // optional, buyAsset per sellAsset
  "priceCeiling": "0.0",                    // optional
  "strategy": "TWAP",                       // TWAP | VWAP | ADAPTIVE
  "venueAllowlist": ["cspr.trade"],
  "nonce": "<unique>"
}
```

The treasurer signs the typed-data representation with `casper-eip-712`. The vault stores `keccak/hash(mandate)` and verifies the signature on funding. Mandate fields, once stored, are immutable for the life of the vault instance.

## 4. Execution Vault contract (Odra)

Build contracts against the Odra framework — read `https://odra.dev/llms.txt` and `https://odra.dev/docs/` before editing.

### State
- `owner` / `treasury` identity
- `mandate_hash` and decoded limits (`total_sell`, `end_time`, `max_slippage_bps`, `price_floor`, `price_ceiling`, `venue_allowlist`)
- `sold_so_far`, `bought_so_far`, `slice_count`
- `agent_identity` (the account-abstraction identity authorised to call `execute_slice`)
- `status`: `Funded | Active | Paused | Completed | Expired | Halted`
- optional cross-contract wiring (all unset by default): `oracle` band check,
  `venue_is_adapter` settlement routing, and `fee_module` + `fee_active` +
  `pending_fee_base` for decoupled protocol-fee accrual

### Entrypoints (illustrative)
| Entrypoint | Caller | Behaviour |
|---|---|---|
| `init(mandate, signature)` | treasury | Verify signature, store hash + limits, set `Funded` |
| `fund()` | treasury | Receive the sell asset into custody, set `Active` |
| `execute_slice(quote, route, min_out)` | agent identity | Re-validate against limits; if any check fails → **revert**; else perform/settle the swap, update totals, record fill |
| `record_fill(slice_id, bought, swap_ref)` | agent identity | Credit realised proceeds for an off-chain/escrow slice (enforces `bought >= min_out`); accumulates the optional fee obligation locally |
| `attest(slice_id, decision_blob)` | agent identity | Record the decision attestation for a slice |
| `pause()` / `resume()` | agent / treasury / GUARDIAN | Circuit-breaker control; **idempotent** (a no-op when already in the target state) so a desk-wide Guardian sweep tolerates status drift |
| `settle()` | anyone after `end_time` or on completion | Release proceeds to treasury, set `Completed`/`Expired`, emit final report |
| `emergency_withdraw()` | treasury (only while `Paused`) | Incident kill-switch: sweep remaining balance to treasury, set terminal `Halted` |
| `set_guardian` / `set_venue_adapter` / `set_oracle` | treasury | Optional wiring: GUARDIAN role, cross-contract `VenueAdapter` settlement, oracle price-deviation band |
| `set_fee_module` / `unset_fee_module` | treasury | Enable/disable optional protocol-fee accrual (unset by default) |
| `flush_fees()` | agent / treasury | Push the accumulated fee obligation to the fee module; the **only** place the external `accrue_fee` call happens |

**Optional protocol fee (decoupled, fail-safe).** Fee accrual is intentionally
split from fill recording: a recorded fill only accumulates its realised buy amount
into `pending_fee_base` locally (no external call), and the cross-contract
`accrue_fee` push happens solely in the separate, retriable `flush_fees`. This
guarantees a fee-module fault (revoked collector role, a repointed/buggy module)
can never block a legitimate, already-settled fill — it can only fail the
independent `flush_fees`, leaving the obligation intact for retry (CLAUDE.md
§4.5/§4.6). Unset by default, so vaults without a fee module are unaffected.

### On-chain checks in `execute_slice` (all must pass)
1. `status == Active`
2. `now <= end_time`
3. `sold_so_far + slice_size <= total_sell`
4. realised price within `[price_floor, price_ceiling]` (if set)
5. effective slippage `<= max_slippage_bps` (quote vs `min_out`)
6. venue ∈ `venue_allowlist`
7. caller == `agent_identity`

The sell asset is released **only** to the venue's mandate-bound address, stored at
`init` and keyed by venue name. The caller does not supply the destination, so the
agent cannot redirect funds to an address it controls even with an allowlisted
venue name. If any check fails the call reverts — this is the guardrail the agent
cannot bypass.

## 5. The agent service

### Execution loop (canonical — keep order intact)
1. Read active mandate/state from the vault (Casper MCP).
2. **Planner** proposes the next child order from mandate + market state.
3. **Executor** fetches a fresh quote/route from CSPR.trade MCP.
   - If the decision requires premium depth/volatility data, pay via **x402** and log the proof.
4. Executor pre-validates the proposal against the mandate (local check).
5. Submit `execute_slice`; the **vault re-validates on-chain** and executes or reverts.
6. Write the decision **attestation** on-chain.
7. Emit event → dashboard updates via streaming.
8. If window closed / order complete → `settle()` and final report. Else loop.

### Planner (LLM)
- Input: mandate + recent market snapshot + remaining size + elapsed time.
- Output: a strictly-typed proposal `{ sliceSize, notBefore, maxSlippageBps, reason }`.
- The planner must return JSON only and never request actions outside the mandate. The executor treats planner output as **untrusted input** and validates everything.
- **Strategy registry** (`agent/src/planner/strategies/`): a pure, deterministic function per mandate strategy produces a *reference* slice size + suggested slippage that guides the planner. `TWAP` splits the remainder evenly; `VWAP` caps the slice to a participation rate of observed sell-side depth (falls back to TWAP when depth is unknown); `ADAPTIVE` shrinks the slice and tightens slippage inversely to volatility. These are guidance only — never authority.

### Executor (deterministic)
- No randomness, no LLM calls. Pure validation + submission + retry/backoff.
- Pauses (does not guess) on: quote failure, abnormal volatility, repeated reverts, or missing data.
- **Circuit breaker** (`agent/src/executor/circuit-breaker/`): a pure state machine consulted before each slice. It trips **open** (pausing the vault and stopping, funds left safe for later settlement) when volatility reaches the trip threshold or after N consecutive non-fills, and re-closes only once volatility falls back below a lower reset threshold (hysteresis). Volatility is taken from purchased x402 data, or estimated as realised volatility from the agent's own mid-price samples when none is purchased — never fabricated.

### Portfolio (concurrent mandates)
- **One vault per mandate** keeps custody isolated; the portfolio layer (`agent/src/portfolio/`) lives entirely in the agent and never commingles funds.
- A pure, deterministic **scheduler** (`scheduler.ts`) selects the next mandate to act on by *time pressure* — remaining size per millisecond left until deadline — with deadline then id tie-breaks, so selection is reproducible. The immutable **`Portfolio`** (`manager.ts`) holds each mandate's track and returns a new instance on every state advance.
- A **manifest** (`manifest.ts`, validated with Zod; unique vault hashes enforced) lists `{ signedMandatePath, vaultContractHash, treasuryAccountHash? }` per mandate. Set `PORTFOLIO_MANIFEST_PATH` to run `runPortfolio` instead of the single-mandate loop. Each mandate keeps its own circuit breaker; paused tracks are left safe for later settlement.
- **Multi-pair.** Each mandate carries its own `sellAsset`/`buyAsset` (`RuntimeMandate`), so a portfolio can execute several pairs concurrently. The executor quotes and swaps each mandate's own pair, and realised volatility is tracked in a separate price window per pair. (The dashboard's portfolio table denominates figures in the configured `VITE_SELL_ASSET`/`VITE_BUY_ASSET` — accurate for a single-pair portfolio; per-vault asset labels for mixed pairs need the asset symbols emitted on-chain, a contract follow-up.)

## 6. Toolkit integration details

### CSPR.trade MCP — quotes, routes, swaps (`https://mcp.cspr.trade`)
Use it to get a quote (`get_quote`-style call with `token_in`, `token_out`, `amount`, `type`) and to execute the resulting route. The quote's `min_received` / price-impact fields feed the executor's slippage pre-check; the contract independently enforces `min_out`.

### x402 Facilitator — premium data (`https://docs.cspr.cloud/x402-facilitator-api/reference`)
Pattern (HTTP 402 challenge → pay → retry with proof):
```
GET /api/v1/market-depth → 402 Payment Required
  X-Payment-Address, X-Payment-Amount, X-Payment-Network: casper
Agent signs payment, retries:
GET /api/v1/market-depth
  X-Payment: casper:<addr>:<amount>:<sig>
→ 200 OK { depth, volatility, ... }
```
Log every payment proof; surface it in the demo. Reference examples: `https://github.com/make-software/casper-x402/tree/master/examples`.

### Casper MCP Server — reads (`https://docs.cspr.cloud/agentic-tools/mcp-server`)
Balances, deploy status, contract state. Used to read vault state and confirm swap settlement.

### CSPR.cloud streaming — dashboard
Subscribe to vault events (fills, attestations, status changes) and render live. Skill: `https://cspr.cloud/skill.md`.

### casper-eip-712 — mandate signing (`https://github.com/casper-ecosystem/casper-eip-712`)
Define the mandate typed-data domain + types; sign in the UI; verify in the vault `init`.

### Account abstraction
The agent runs under its own on-chain identity authorised in the vault as `agent_identity`. After the mandate is signed, no human wallet signs execution transactions.

## 7. Setup & run

> The `.env` values below are real configuration supplied per environment (node RPC, API keys, the agent identity key). They are the only blanks — there are no placeholder values, mock services, or stubbed responses anywhere in the codebase; the prototype runs against live Casper Testnet and the real CSPR.trade / x402 endpoints.

### Prerequisites
- Rust toolchain + Odra; Node 18+ ; a Casper Testnet account with test CSPR.
- Environment (`.env`, git-ignored), supplied per machine:
  ```
  CASPER_NODE_RPC=<testnet node rpc url>
  CSPR_CLOUD_API_KEY=<your cspr.cloud key>
  CSPR_TRADE_MCP_URL=https://mcp.cspr.trade
  X402_FACILITATOR_URL=<casper x402 facilitator url>
  AGENT_PRIVATE_KEY=<agent identity key, never committed>
  LLM_API_KEY=<planner model key>
  ```

### Steps
1. **Contracts:** build and deploy the Execution Vault to Casper Testnet; note the contract hash.
2. **Agent:** `npm install` in `/agent`; configure `.env`; `npm run dev`.
3. **Dashboard:** `npm install` and `npm run dev` in `/dashboard`.
4. **Demo:** `npm run demo` — signs a real mandate, funds the vault, runs the agent against the configured testnet pair.

## 8. Testing & verification
- **Unit:** planner output schema; executor guardrail pre-checks; mandate signing/verification.
- **Contract:** every check in §4.7 has a test proving an out-of-bounds attempt **reverts** (over-cap, past-deadline, over-slippage, wrong venue, wrong caller).
- **Integration:** full loop on testnet for a small order; confirm fills, attestations, and settlement on the explorer.
- **Demo rehearsal:** capture explorer links and one x402 payment proof; rehearse the blocked out-of-bounds attempt.

## 9. UI & dashboard design

The dashboard is the treasurer's control surface. It must feel calm and trustworthy for someone moving real funds, not like a dense trading terminal.

### Principles
- **Simple and user-focused.** Three screens only: **(1) Create & sign mandate**, **(2) Live execution**, **(3) Final report**. Each screen answers one question — *what am I authorising / what is happening now / what happened*. Do not add screens or panels that don't serve those questions.
- **No gradients anywhere.** Solid fills, clear borders, restrained palette. No gradient backgrounds, buttons, charts, or accents. State is conveyed with a solid colour plus a text label — filled (green), pending (neutral), paused (amber), blocked (red) — never with gradient effects.
- **Clarity over density.** The live screen leads with four large, readable figures: remaining size, average price, slippage saved, time left. Everything else (per-slice list, route details, attestations) sits behind a details view.
- **Plain language.** Render the mandate as a human sentence next to the technical fields, e.g. "Sell 2,000,000 CSPR for USDC by Friday 18:00 UTC, never worse than 1% slippage."
- **Trust through transparency.** Every fill links to the testnet explorer; each slice shows the agent's stated reason; a blocked out-of-bounds attempt is shown explicitly as proof the guardrails hold.
- **Real states only.** Loading, empty, paused, and error states are designed deliberately and bound to real chain/agent state. The UI never renders sample or placeholder numbers.
- **Accessible & responsive.** Sufficient contrast, keyboard-usable controls, works on laptop and phone.

### Screens
1. **Create & sign mandate** — a short form (sell/buy asset, total size, window, max slippage, optional price band) with inline validation, a plain-language summary of what will be authorised, and the gasless EIP-712 signing action.
2. **Live execution** — the four headline figures, a progress indicator for the order, a live slice feed (each with status, price, explorer link, and reason), the most recent x402 data-purchase proof, and pause/resume controls.
3. **Final report** — average price, totals, slice count, fees, and slippage saved vs the naive baseline, with a link to the full on-chain audit trail.

Plus a desk-wide **Portfolio** view (when several mandates run concurrently): aggregate figures (mandate count, active, total remaining, aggregate average price, nearest deadline) over a per-vault table. It streams one WebSocket per vault (`VITE_VAULT_CONTRACT_HASHES`, comma-separated) and reconstructs each vault's state independently — same real-states-only discipline.

### Data source
All dashboard data derives from real chain state and agent events via CSPR.cloud streaming (§6). There is no mock layer; if state is unavailable, the UI shows an honest loading or error state rather than fabricated values.

## 10. Security model
- **Custody:** only the vault holds funds; the agent service holds none. Swap proceeds are delivered to the treasury (`TREASURY_ACCOUNT_HASH`), never to the agent — the agent is never a recipient of funds.
- **Authority:** the vault is the final authority; the LLM has none and never holds keys.
- **Replay:** mandate `nonce` + one-shot `init` prevent re-use.
- **Liveness:** if the agent dies mid-order, funds remain safe in the vault and `settle()` can be called after `end_time` by anyone to return remaining funds to the treasury.
- **Secrets:** environment-only; never logged or committed.

## 11. Metrics & reporting
The final report (emitted on settle) includes: realised average price, total sold/bought, number of slices, fees, and **slippage saved vs a naive single-market-order baseline** computed on the same pool. This number is the headline proof of value in the demo.

## 12. Roadmap (post-buildathon)
- ~~VWAP and adaptive strategies; volatility-aware circuit breaker.~~ **Implemented** — see the strategy registry (`agent/src/planner/strategies/`) and circuit breaker (`agent/src/executor/circuit-breaker/`) in §5.
- Multi-venue smart routing and best-execution across pools.
- ~~Multi-mandate **and multi-pair** portfolio management.~~ **Implemented** — the portfolio scheduler/manager (`agent/src/portfolio/`) runs several mandates concurrently, each with its own sell/buy pair (§5). Remaining: emitting asset symbols on-chain so the dashboard can label mixed-pair portfolios per vault.
- Mandate registry + role-based treasury permissions.
- Cross-chain execution once supporting infrastructure is available.

## 13. Glossary
- **Mandate** — signed, off-chain order authorisation defining what may be executed and the limits.
- **Child order / slice** — one piece of the parent order executed at a point in time.
- **Guardrail** — an on-chain limit enforced by the vault (cap, deadline, slippage, price band, venue, caller).
- **Attestation** — on-chain record of an agent decision tied to a slice, for auditability.
- **TWAP/VWAP** — time-/volume-weighted average price execution strategies.
- **Account abstraction** — the agent's own on-chain identity, used to act without a human wallet in the loop.