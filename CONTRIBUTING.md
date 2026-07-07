# Contributing to Cadence

Thanks for your interest in Cadence — an agentic OTC execution desk on Casper.
This guide covers how to get set up, the checks your change must pass, and the
conventions the codebase follows.

## Ground rules

- **The contract is the source of truth.** Any change to execution limits must
  be enforced on-chain in the vault's `execute_slice`, not only in the
  off-chain executor. Off-chain checks are an optimization, never the authority.
- **No mock data.** The dashboard and agent read real chain state. When
  unconfigured or disconnected, show honest loading/empty/error states — never
  sample numbers, placeholders, or gradients.
- Keep changes focused and match the style of the surrounding code.

## Getting set up

Prerequisites: Rust + [Odra](https://odra.dev) (`cargo install cargo-odra`),
Node 18+ (CI uses Node 22), and — for on-chain work — a Casper Testnet account
with test CSPR.

```bash
npm install              # installs all workspaces
cp .env.example .env     # fill in node RPC, API keys, agent/treasury keys
```

## Checks your change must pass

CI runs these on every pull request; run them locally first.

TypeScript (mandate / agent / scripts / dashboard):

```bash
npm run typecheck
npm test
npm run build
```

Rust / Odra contracts:

```bash
cd contracts
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash build-wasm.sh       # produces deployable Casper WASM per crate
```

## Pull requests

- Branch off `main`; keep PRs scoped to one logical change.
- Fill in the PR template (what changed, why, how it was verified).
- Every guardrail or math change needs a test that would fail without it.
- Do not commit secrets. Update `.env.example` when you add a new variable.
- Update the `README.md` / `docs/` when behavior or setup changes.

## Reporting security issues

Do **not** open a public issue for a vulnerability — see
[`SECURITY.md`](SECURITY.md) for private reporting.

## Code of conduct

Participation is governed by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
