<!-- Keep PRs scoped to one logical change. -->

## What & why

<!-- What does this change do, and why is it needed? -->

## How it was verified

<!-- Commands run and what you observed. Check what applies. -->

- [ ] `npm run typecheck && npm test && npm run build`
- [ ] `cd contracts && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- [ ] Exercised on testnet (link the tx/explorer output if applicable)

## Checklist

- [ ] Any change to execution limits is enforced on-chain in the vault, not only off-chain
- [ ] New/changed guardrail or math has a test that fails without this change
- [ ] No mock data / placeholders / gradients added
- [ ] No secrets committed; `.env.example` updated if a new variable was added
- [ ] Docs/README updated if behavior or setup changed
