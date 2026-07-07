# Security Policy

Cadence moves funds on-chain, so we take security seriously. This document
explains what is in scope, how to report a vulnerability, and the trust model
you should assume when reviewing the code.

## Trust model (read this first)

The core safety property is that **the contract is the source of truth**. Every
limit from a signed mandate — spend cap, deadline, per-slice slippage, price
band, venue allowlist and caller authority — is re-validated on-chain in the
Execution Vault's `execute_slice`. If any check fails, the call reverts. The
off-chain agent (including its LLM planner) can only *propose* trades; it can
never breach an on-chain limit, and a hallucinated plan is physically incapable
of exceeding the signed mandate.

A vulnerability is therefore most severe when it lets a caller move funds
outside the mandate's limits, replay a mandate, or bypass an on-chain guardrail.

## Supported versions

This is an active buildathon prototype. Security fixes are applied to `main`
only; there are no long-term support branches.

## Reporting a vulnerability

**Please do not open a public issue for a security vulnerability.**

Use GitHub's private reporting instead:

1. Go to the repository's **Security** tab → **Report a vulnerability**
   (GitHub Private Vulnerability Reporting).
2. Describe the issue, the affected component (vault / agent / mandate /
   dashboard), and a proof-of-concept or reproduction if you have one.

If private reporting is unavailable, contact the maintainers through the
Casper developer channels linked in the README rather than filing a public
issue.

We aim to acknowledge a report within a few days and will coordinate a fix and
disclosure timeline with you.

## Scope

In scope:

- The Odra contracts in `contracts/` — especially the Execution Vault
  guardrails, mandate binding/replay protection, and the swap adapter.
- Mandate signing/verification (`mandate/`) — EIP-712 digest derivation,
  signature verification, tamper and replay handling.
- The deterministic executor's guardrail parity with the contract
  (`agent/src/executor/`).

Out of scope:

- Third-party services (CSPR.cloud, CSPR.trade MCP, the x402 facilitator, the
  LLM provider) and their availability.
- Findings that require a compromised operator key or node — key custody is the
  operator's responsibility.
- Denial of service from network-level flooding.

## Handling of secrets

No private keys or API secrets are committed. `.env.example` documents the
required variables; real values live only in an untracked `.env`. If you find a
committed secret, report it privately as above.
