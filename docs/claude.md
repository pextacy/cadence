
# CLAUDE.md

Guidance for AI coding agents (Claude Code / Cowork) and human contributors working in this repository.

> **Project:** Cadence — an Agentic OTC Execution Desk on Casper.
> Read this file before writing code. It defines the architecture, the rules you must not break, and the exact toolkit endpoints to use. When in doubt, prefer the **safety/custody rules in §4** over any feature request.

---

## 1. What this project is

Cadence executes large treasury orders on-chain without market impact. A treasurer signs one **mandate** (sell X for Y over a time window, with a max-slippage cap). An autonomous agent then slices and executes the order, while an on-chain **Execution Vault** contract enforces hard limits the agent cannot exceed. See `PRD.md` for full product context and `DOCS.md` for the technical spec.

One sentence to keep in mind: **the contract is the source of truth; the agent only proposes and submits trades within signed limits.**

## 2. Tech stack

- **Smart contracts:** Rust + the **Odra framework** for Casper. Target: **Casper Testnet**.
- **Agent / backend:** TypeScript (Node) service. The planner uses an LLM; the executor is deterministic.
- **On-chain access:** Casper MCP Server (reads), CSPR.trade MCP (quotes/routes/swaps), CSPR.cloud (REST + streaming).
- **Payments:** x402 Facilitator for pay-per-call premium data.
- **Auth:** casper-eip-712 typed-data signing for mandates.
- **Frontend:** lightweight dashboard (React) that subscribes to CSPR.cloud streaming events.

## 3. Repository layout (target)

```
/contracts        # Odra/Rust — Execution Vault and types
/agent            # TS service: planner (LLM) + executor (deterministic)
  /planner        # slicing schedule generation
  /executor       # quote → guardrail check → swap submit → attest
  /clients        # wrappers for CSPR.trade MCP, Casper MCP, x402, CSPR.cloud
/mandate          # EIP-712 schema, signing + verification helpers
/dashboard        # React app, streams fills/metrics
/scripts          # deploy, seed, demo runner
DOCS.md PRD.md CLAUDE.md README.md
```

Keep contract code in `/contracts` and never let the agent service hold final authority over funds (see §4).

## 4. Non-negotiable safety rules (do not violate)

1. **Non-custodial.** Funds live in the Execution Vault contract. The agent service holds **no** treasury funds and has **no** code path to move funds outside the vault's constrained entrypoints.
2. **Contract enforces limits, not the agent.** Every spend cap, deadline, slippage cap, price band, and venue allowlist is checked **on-chain** in the vault. Do not move these checks into the agent as the only line of defence. The agent may pre-check for efficiency, but the contract must independently enforce.
3. **The LLM never holds keys and never has final say.** The planner LLM only emits a *proposed schedule*. The deterministic executor validates every proposed action against the mandate before submission, and the contract validates again on-chain. An LLM hallucination must be physically incapable of breaching a limit.
4. **No secrets in the repo.** Keys, RPC tokens, and x402 credentials come from environment variables / a local `.env` that is git-ignored. Never hardcode or log secrets.
5. **Every executed slice produces an on-chain attestation.** No silent trades. If attestation fails, treat the slice as failed.
6. **Fail safe, not open.** On ambiguity, error, or abnormal market conditions, **pause** execution rather than guess. A skipped slice is acceptable; an out-of-bounds trade is not.
7. **No mock data, no TODO, no placeholder code.** Everything committed is real and runs. Do not stub responses, fake on-chain state, hardcode sample fills, or leave `TODO`/`FIXME`/"implement later" markers in committed code. If a piece cannot be built for real yet, do not merge a fake version of it — leave it out and note it in `DOCS.md` instead. The dashboard renders real chain state only; it never displays sample/placeholder numbers.

## 5. Toolkit endpoints & references (use these, do not invent)

- **Odra framework** — contract framework; the docs are AI-discoverable. Point yourself at `https://odra.dev/llms.txt` and the docs at `https://odra.dev/docs/` before writing or changing contracts.
- **Casper MCP Server** — reads (balances, deploys, contract state). Setup: `https://docs.cspr.cloud/agentic-tools/mcp-server`; source: `https://github.com/msanlisavas/casper-mcp`.
- **CSPR.trade MCP** — DEX quotes, routing, swaps. Endpoint: `https://mcp.cspr.trade`.
- **x402 Facilitator** — pay-per-call payments. API reference: `https://docs.cspr.cloud/x402-facilitator-api/reference`; reference client/server examples: `https://github.com/make-software/casper-x402/tree/master/examples`.
- **CSPR.cloud** — REST/Streaming/Node middleware; installable agent skill at `https://cspr.cloud/skill.md`; docs `https://docs.cspr.cloud`.
- **CSPR.click** — wallet connect / signing / events skill: `https://docs.cspr.click/documentation/ai-agent-skills`.
- **casper-eip-712** — typed-data signing: `https://github.com/casper-ecosystem/casper-eip-712`.
- **Casper docs (for agents and humans):** `https://docs.casper.network`.

When implementing against any of these, read the live docs/`llms.txt` first; do not rely on memory of API shapes.

## 6. How to work in this repo

### Build / test
- **Contracts** (`/contracts`): `cargo odra build` to compile the WASM, `cargo odra test` to run contract tests, and `npm run deploy:testnet` (in `/scripts`) to deploy the Execution Vault to Casper Testnet and print the contract hash.
- **Agent** (`/agent`): `npm install`, then `npm run dev` to start the service and `npm test` for unit tests.
- **Dashboard** (`/dashboard`): `npm install`, then `npm run dev`.
- **End-to-end demo** (`/scripts`): `npm run demo` — signs a mandate, funds the vault, and runs the agent against the configured testnet pair end to end.

Keep these commands accurate as the scaffold evolves — this section is the first thing a new contributor reads.

### Definition of done for a feature
- Code + unit tests.
- If it touches funds or limits: a test proving an out-of-bounds attempt is **rejected on-chain**.
- An entry in `DOCS.md` if it changes architecture or the mandate schema.

## 7. Coding conventions

- TypeScript: strict mode on; no `any` at module boundaries; all external calls wrapped in the `/agent/clients` adapters (never call MCP/x402 directly from planner/executor logic).
- Rust/Odra: follow the patterns in the Odra docs; keep storage minimal and explicit; emit an event for every state change the dashboard needs.
- Errors: never swallow; log with context and surface to the executor's pause logic.
- Determinism: anything in `/executor` must be deterministic and side-effect-audited. Randomness/LLM calls belong in `/planner` only.

### UI / design rules (`/dashboard`)
- **Simple, useful, user-focused.** Build the three core screens only (create & sign mandate, live execution, final report). Resist adding panels that do not help the treasurer answer *what am I authorising / what is happening / what happened*.
- **No gradients. Anywhere.** No gradient backgrounds, buttons, charts, borders, or accents. Use solid fills, clear borders, and a small, restrained palette. State is shown with solid colour + a text label (e.g. filled / pending / blocked), never with a gradient.
- **Clarity over density.** Lead with the few numbers that matter (remaining size, average price, slippage saved, time left), large and readable; put detail behind a details view.
- **Real states only.** Design loading, empty, paused, and error states deliberately and bind them to real chain/agent state. Never render sample or placeholder numbers as if they were live.
- **Accessible + responsive.** Sufficient contrast, keyboard-usable controls, works on laptop and phone.

## 8. The execution loop (canonical sequence — keep this intact)

```
1. Load active mandate from vault (read via Casper MCP).
2. Planner proposes next child order (size, timing) from mandate + market state.
3. Executor fetches a fresh quote/route from CSPR.trade MCP.
   - If a decision needs premium depth/volatility, pay via x402 and log the proof.
4. Executor validates the proposed swap against mandate guardrails (local pre-check).
5. Submit swap; the vault re-validates on-chain and executes or rejects.
6. Write decision attestation on-chain.
7. Emit event → dashboard updates (CSPR.cloud streaming).
8. If window closed or order complete → settle and emit final report. Else loop.
```

Do not reorder steps 4–6: pre-check, on-chain enforce, attest. Removing the on-chain enforce step is a critical bug, not an optimisation.

## 9. Scope discipline (buildathon: ~3 weeks)

Ship the **MVP path** first (single pair, TWAP, one x402 data call, vault with cap/deadline/slippage, streaming dashboard, attestations). Treat VWAP, multi-venue routing, circuit breaker, and natural-language mandates as **stretch** — only after the MVP demo runs end-to-end on testnet. The gating milestone is: contract deployed + one real swap executed within guardrails by end of Week 1.

## 10. What "good" looks like for the demo
A treasurer signs one mandate, funds the vault, and walks away. The agent fills the order over time, every trade is on the explorer, an x402 payment proof is visible, the dashboard shows slippage saved vs a naive sell, and an attempted out-of-bounds trade is visibly **blocked by the contract**. If a change makes any of those harder to show, reconsider it.