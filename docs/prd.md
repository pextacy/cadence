# Cadence — Product Requirements Document (PRD)

**Project:** Cadence — an Agentic OTC Execution Desk on Casper
**Event:** Casper Agentic Buildathon 2026 (Qualification Round, June 1–30)
**Track:** Casper Innovation Track
**Status:** Draft v1.0
**Owner:** Cadence Team

---

## 1. Summary

Cadence is an autonomous **OTC execution desk** for on-chain treasuries. A treasury owner signs a single, human-readable **execution mandate** (e.g. "sell 2,000,000 CSPR for USDC over 3 days, never accept more than 1% slippage"). An autonomous AI agent then plans and executes that order on its own: it slices the parent order into child orders, monitors live market conditions, buys premium market-depth data when it needs it, and executes swaps — all inside hard on-chain guardrails enforced by a smart contract the agent cannot override.

The product turns a manual, slippage-prone, attention-heavy task — moving size without moving the market — into a self-driving, non-custodial, auditable workflow.

## 2. Problem

Treasuries (DAOs, funds, protocols, market makers) regularly need to convert large positions. Doing this naively causes three real losses:

- **Slippage and market impact.** A single large market order walks the book and the treasury sells into its own price impact.
- **Operator burden.** Splitting an order across time and venues to minimise impact requires constant attention; a human babysits a terminal for days.
- **Trust and auditability gaps.** Delegating execution to a bot or OTC counterparty usually means handing over custody or trusting an opaque process with no on-chain record of *why* each trade happened.

Today, sophisticated execution (TWAP/VWAP slicing, smart routing, impact control) is the domain of centralised desks and institutional tooling. On-chain treasuries lack a non-custodial, agent-driven equivalent.

## 3. Goals & non-goals

### Goals
1. Let a treasury authorise execution with **one signed mandate**, no per-trade approvals.
2. Execute large orders with **measurably lower slippage** than a single naive market order.
3. Keep funds **non-custodial**: an on-chain contract enforces all limits; the agent can never exceed them.
4. Produce a **complete, on-chain audit trail** — every decision and fill is recorded and verifiable.
5. Demonstrate **meaningful, deep use of the Casper AI Toolkit** (x402, CSPR.trade MCP, Odra, account abstraction, EIP-712 signing, streaming).

### Non-goals (for the buildathon MVP)
- Not a general spot exchange or wallet.
- No leverage, derivatives, or cross-chain execution in the MVP.
- No fiat on/off-ramp.
- Not multi-tenant/production-hardened — a working testnet prototype with one or two trading pairs is the target.

## 4. Target users & use cases

| User | Job to be done |
|------|----------------|
| DAO treasurer | Diversify treasury from native token into stablecoins without crashing the price |
| Crypto fund / market maker | Build or unwind a position over a defined window with an impact budget |
| Protocol operator | Convert protocol revenue on a schedule, hands-off |

Primary use case for the demo: a DAO treasurer converting a large CSPR position to a stablecoin over a multi-day window with a strict slippage cap.

## 5. Product overview

### 5.1 Core flow
1. **Define mandate.** Treasurer fills a form: sell asset, buy asset, total size, time window, max slippage %, optional price floor/ceiling.
2. **Sign mandate.** Treasurer signs the mandate as EIP-712-style typed data (human-readable, gasless off-chain signature).
3. **Fund vault.** Treasurer funds the on-chain Execution Vault contract with the sell asset. The vault records the signed mandate hash and constraints.
4. **Agent plans.** The planner agent reads the mandate and current market state and produces a slicing schedule (size and timing of child orders).
5. **Agent executes.** For each child order, the agent fetches a fresh quote/route, checks it against the guardrails, executes the swap, and records the fill + its reasoning on-chain.
6. **Monitor.** A live dashboard streams fills, remaining size, realised average price, and slippage saved.
7. **Settle & report.** When the window closes or the order completes, the vault releases proceeds to the treasury and emits a final execution report.

### 5.2 The guardrail principle
The smart contract — not the agent — is the source of truth. The agent can only *propose and submit* trades; the vault rejects anything that violates the signed mandate (over the spend cap, past the deadline, worse than max slippage, outside the price band, or to a non-allowlisted venue). This is what makes delegation safe.

### 5.3 UX & design principles
The interface must feel like a calm, trustworthy control surface for someone moving real money — not a busy trading terminal. The bar is: a treasurer should understand the whole state at a glance and never feel unsure about what the agent is doing with their funds.

- **Simple and user-focused.** Three screens, no more: (1) create & sign a mandate, (2) live execution dashboard, (3) final report. Each screen answers one question — *what am I authorising / what is happening now / what happened*.
- **Clarity over density.** Show the few numbers that matter (remaining size, average price, slippage saved, time left) prominently; tuck everything else behind a details view. Generous whitespace, clear hierarchy, large readable numbers.
- **Plain language, no jargon walls.** Explain the mandate in a human sentence ("Sell 2,000,000 CSPR for USDC by Friday, never worse than 1% slippage") alongside the technical fields.
- **Trust through transparency.** Every fill links to the testnet explorer; the agent's reasoning for each slice is visible; a blocked out-of-bounds attempt is shown plainly as proof the guardrails work.
- **Flat, solid styling — no gradients anywhere.** Use solid fills, clear borders, and a restrained palette. No gradient backgrounds, buttons, charts, or accents. Communicate state with solid colour and clear labels (e.g. green = filled, neutral = pending, red = blocked), not with gradient effects.
- **Honest states.** Loading, empty, paused, and error states are designed deliberately and show what is actually true — never fake/placeholder data standing in for real state.
- **Accessible defaults.** Sufficient contrast, keyboard-usable controls, responsive layout that works on a laptop and a phone.

## 6. Functional requirements

### MVP (must-have for submission)
- **FR-1** Mandate creation UI with validation.
- **FR-2** EIP-712-style typed-data signing of the mandate (no gas to authorise).
- **FR-3** Execution Vault contract (Odra/Rust) that: custodies the sell asset, stores the mandate hash + constraints, verifies the signature, exposes a constrained `execute_slice` entrypoint, enforces total spend cap, deadline, max slippage, and venue allowlist, and releases proceeds on completion/expiry.
- **FR-4** Planner agent that produces a TWAP-style slicing schedule from the mandate and adapts it to market conditions.
- **FR-5** Executor that fetches quotes/routes via the **CSPR.trade MCP** server and submits swaps within guardrails, with retry/backoff.
- **FR-6** At least one **x402**-paid premium data call (e.g. market-depth / volatility feed) used in a real decision, with the payment proof logged.
- **FR-7** On-chain **decision attestation** for every child order (inputs, chosen route, expected vs realised price, reason).
- **FR-8** Live dashboard via **CSPR.cloud streaming** showing fills, remaining size, and running slippage-saved metric.
- **FR-9** Final execution report (average price, total slippage vs naive baseline, number of slices, fees).

### Should-have (if time allows)
- **FR-10** Circuit breaker: pause execution on abnormal volatility, resume when stable.
- **FR-11** VWAP mode (volume-weighted) in addition to TWAP.
- **FR-12** Natural-language mandate entry ("sell 2M CSPR over 3 days, cap slippage at 1%").

### Could-have (post-buildathon)
- Multi-venue smart routing, multi-pair, multi-mandate portfolio, cross-chain, production multi-tenant deployment.

## 7. Non-functional requirements
- **Security/custody:** non-custodial; no path for the agent to move funds outside mandate bounds. All limits enforced on-chain.
- **Auditability:** every agent action maps to an on-chain record; the dashboard can reconstruct the full history from chain state.
- **Determinism at the boundary:** the LLM plans; a deterministic executor and the contract enforce. The LLM never holds keys or final authority.
- **Resilience:** swap submission retries with backoff; failed slices are skipped, not silently dropped.
- **Predictable cost:** rely on Casper's fixed-fee model so the agent can budget transactions.
- **Usability:** a non-expert treasurer can authorise and monitor an order without reading the docs; the interface is flat and solid-styled with no gradients.
- **No placeholders in shipped work:** the prototype shows real on-chain state and runs real swaps on testnet — no mock data, stubbed responses, or TODO/placeholder code in any submitted build.

## 8. Architecture (high level)

```
            sign (EIP-712 typed data, gasless)
Treasurer ───────────────────────────────────► Mandate
   │ fund                                          │
   ▼                                               ▼
┌─────────────────────────┐   verifies sig + limits   ┌───────────────────┐
│  Execution Vault (Odra)  │◄──────────────────────────│   Cadence Agent │
│  - custody               │   execute_slice() within  │  - Planner (LLM)   │
│  - mandate hash + limits │   guardrails              │  - Executor (det.) │
│  - fills + attestations  │──────────────────────────►│                    │
└─────────────────────────┘        emits events        └─────────┬─────────┘
            │ stream (SSE / CSPR.cloud)                           │
            ▼                                                     │ quotes/routes/swaps
      Live Dashboard                                              ▼
                                                        ┌───────────────────┐
   premium depth/volatility (x402, pay-per-call) ◄──────│  CSPR.trade MCP    │
                                                        │  Casper MCP (read) │
                                                        └───────────────────┘
```

The desk agent operates under its **own on-chain identity** (account abstraction); there is no human wallet in the execution loop after the mandate is signed.

## 9. Casper AI Toolkit usage map

| Toolkit component | Role in Cadence |
|---|---|
| **CSPR.trade MCP** | Primary execution: quotes, routing, and swap submission for each child order |
| **x402 Facilitator** | Pay-per-call premium market-depth/volatility data used in slicing decisions |
| **Odra Framework** | The Execution Vault smart contract (custody + guardrails + fills + attestation) |
| **Account abstraction** | The desk agent acts under its own on-chain identity, no human wallet in the loop |
| **casper-eip-712** | Gasless, human-readable mandate authorisation; verified on-chain by the vault |
| **CSPR.cloud + streaming** | Real-time dashboard of fills, balances, and progress |
| **Casper MCP Server** | Balance and transaction-confirmation reads |

## 10. Success metrics
- **Primary (demo proof):** realised average execution price beats a single naive market sell of the same size; slippage saved is shown as a concrete number.
- Order completes within the mandated window and never breaches the slippage cap.
- 100% of agent decisions have a matching on-chain attestation.
- Zero out-of-bounds actions accepted by the vault (guardrails provably hold).

## 11. Demo plan (2–3 minutes)
1. Treasurer defines and **signs** a mandate (no gas) and funds the vault.
2. Dashboard shows the agent **planning** the slices and explaining its schedule.
3. Trigger execution; watch child swaps **fill live**, each linking to a testnet explorer transaction.
4. Show an **x402 payment** the agent made for premium data, with the payment proof.
5. Close with the **report**: average price, slippage saved vs naive baseline, full audit trail — and a failed/blocked out-of-bounds attempt to prove the guardrails work.

## 12. Milestones (3-week build to June 30)

**Week 1 — Foundations**
- Odra Execution Vault: custody, mandate storage, spend cap, deadline (deploy to Casper Testnet).
- EIP-712 mandate schema + signing/verification.
- CSPR.trade MCP integration: fetch quotes and execute a single test swap.

**Week 2 — The agent**
- Planner (TWAP slicing) + deterministic executor with slippage guardrails and retries.
- x402 premium-data call wired into a real decision.
- On-chain decision attestations.

**Week 3 — Polish & proof**
- Streaming dashboard + final report with slippage-saved metric.
- Guardrail/circuit-breaker tests; record blocked out-of-bounds attempt.
- README, landing page, X account, demo video, submission.

## 13. Mapping to judging criteria
- **Technical Execution / Working Smart Contracts:** Odra vault on testnet enforcing real constraints; deep, organic use of the toolkit (7/7 components).
- **Innovation & Originality:** an agent-run, non-custodial execution desk — a category no other submission occupies.
- **Use of AI / Agentic Systems:** genuine plan→decide→act loop with hard on-chain limits, not a wrapper.
- **Real-World Applicability (DeFi):** solves a concrete, costly treasury problem with a measurable saving.
- **UX & Design:** sign-once mandates and a live execution dashboard.
- **Long-Term Launch Plans / Impact:** clear path from a single pair to multi-venue smart routing; treasuries are a recurring, sticky customer.

## 14. Risks & mitigations
- **Testnet liquidity is thin → slippage numbers look odd.** Mitigation: define the baseline on the same testnet pool so the *comparison* is fair, and document assumptions.
- **Scope creep across 7 toolkit components.** Mitigation: ship one pair, TWAP only, one x402 data call; treat VWAP/routing/NL as stretch.
- **LLM nondeterminism in execution.** Mitigation: LLM only plans; the executor and contract enforce — no LLM decision can breach a limit.
- **Time.** Mitigation: contract + single swap working by end of Week 1 is the gating milestone; everything else builds on it.

## 15. Open questions
- Which testnet stablecoin/pair has enough CSPR.trade liquidity for a convincing demo?
- Single mandate per vault, or a mandate registry from day one?
- Attestation granularity: per child order vs per decision-change — what reads best in the demo?