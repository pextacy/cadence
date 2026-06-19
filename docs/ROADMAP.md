# Cadence — Production Roadmap (ROADMAP.md)

Status of the production-hardening effort, organized as reviewable **waves**. Each
wave must end green: `cd contracts && cargo test --workspace && ./build-wasm.sh`
and `npm run typecheck && npm test` (workspaces) all pass before the next wave
starts.

> Honesty note carried through this whole plan: decomposition and new contracts
> must serve a **real capability**, never a file/LOC target. Where a split would be
> cosmetic it is explicitly avoided.

## Status at a glance

| Wave | Scope | Status |
|------|-------|--------|
| 0 | `cadence-common` shared math | ✅ Done |
| 1 | Decompose every crate by concern + golden-vector preimage tests | ✅ Done |
| 2a | Compose `AccessControl` into the vault (RBAC + `set_guardian`) | ✅ Done |
| 2b | Route `execute_slice` through `VenueAdapter` (atomic path) | ✅ Done (fees + escrow-attestation path remain) |
| **3** | **Guardian desk-wide pause fan-out (cross-contract wiring)** | 🟡 Contracts built; cross-contract wiring/tests pending |
| **4** | **`VaultFactory` + `VaultRegistry` end-to-end create/register flow** | 🟡 Contracts built; deploy-flow integration pending |
| **5** | **`TreasuryMultisig` gating + `OracleAggregator` band cross-check** | 🟡 Contracts built; integration pending |
| 6 | Wire the agent `loop.ts` to persistence/observability/nonce | ✅ Done (on-chain reconciliation + finality-gating remain) |
| X | Cross-cutting: clippy-clean, CI green, E2E, testnet deploy-safety | ⏳ Pending |

Legend: ✅ done · 🟡 components exist & unit-tested but not yet integrated across
contracts · ⏳ not started.

---

## Completed (reference)

- **Wave 0** — `contracts/common` (`cadence-common`): shared fixed-point math
  (`scale`, `checked` U512, `slippage`, `price`, `fee`). Surfaces the real
  `1e9` (vault/oracle) vs `1e6` (dex-adapter) `PRICE_SCALE` split as named consts.
- **Wave 1** — every crate decomposed by audit concern
  (`errors/events/types/preimage/guardrails/storage/lifecycle/execution/admin/views`),
  files <300 LOC, inline tests moved to `tests/`. **Preimage byte layouts frozen
  with golden-vector tests** so the refactor cannot break signature compatibility
  with `mandate/src/sign.ts`.
- **Wave 2a** — vault composes `SubModule<AccessControl>`; `init` bootstraps
  treasury→ROOT_ADMIN+TREASURY+GUARDIAN, agent→AGENT. Auth runs through `has_role`
  but keeps the vault's own error codes. `pause`/`resume` accept a GUARDIAN; a
  treasury-only `set_guardian` wires the role. `init` signature and preimage
  unchanged.
- **Wave 2b** — `execute_slice` settles cross-contract through the `VenueAdapter`
  (`VenueAdapterContractRef::swap` with attached value) for venues opted in via a
  treasury-only `set_venue_adapter`; an atomic `SwapReceipt` records the fill in
  the same call. Backward compatible (direct transfer stays the default).
  `tests/integration_adapter.rs` proves end-to-end atomic settlement to the
  treasury. Found+fixed an Odra by-name cross-contract dispatch bug (adapter param
  names must match the trait). *Remaining:* fee accrual on fill + the
  escrow/signed-attestation path for off-chain (cspr.trade) venues.
- **Wave 6** — `runAgent` uses the ops layer: `FileStateStore` snapshots per tick
  + resume-on-restart, `InProcessMetrics` counters, a hash-chained `FileAuditLog`,
  an opt-in `HealthServer` (`HEALTH_PORT`), and `InProcessNonceManager`
  serialisation. *Remaining:* on-chain state reconciliation on boot and
  finality-gating in the executor (both behind the existing modules).

Current footprint: 13 deployable contracts, ~230 `cargo` tests, 13 wasm artifacts;
agent at 167 TS tests. All green on `main`.

---

## Wave 2b — Route settlement through the VenueAdapter (HIGH risk)

**Goal:** the vault stops doing a blind `transfer_tokens` and settles through the
typed `VenueAdapter` cross-contract interface, with on-chain proof; fees accrue.

**Why it's the crux:** today `execute_slice` (`vault/src/vault/execution.rs`)
releases the sell amount directly to `venue_addr`. cspr.trade is an **off-chain
MCP DEX** (confirmed — no on-chain router callable atomically from wasm), so the
production-honest model is **escrow + signed attestation**, not an atomic swap.

**Steps (open with a spike):**
1. **Spike (do first):** an isolated test proving the vault can build
   `VenueAdapterContractRef::new(env, addr)`, attach value (`.with_tokens(...)`),
   call `swap(...)`, and deserialize the returned `SwapReceipt`. Use
   `Cep18SwapAdapter` (the atomic implementer) for the spike. Prove the Odra
   mechanic before touching the vault's critical path.
2. **Atomic path:** for on-chain venues (`Cep18SwapAdapter`), `execute_slice`
   calls `swap`; if `receipt.atomic`, record the fill immediately from
   `receipt.bought_amount`.
3. **Escrow path:** for off-chain venues (`SettlementAdapter`), `execute_slice`
   escrows the slice and emits intent (`atomic = false`); `record_fill` becomes
   **attestation-gated** — a signed settlement attestation (same
   `verify_signature` discipline as x402) proves the realised buy amount before
   the fill is accepted. Replaces today's unproven `swap_deploy_hash` string.
4. **Fees:** on a recorded fill, call `FeeModule::accrue_fee(buy_asset, bought)`
   cross-contract (`cadence-common::fee` does the bps math).
5. Resolve the adapter address: `venue_addr` now points at an **adapter** contract,
   not a raw destination. Update the venue config semantics + `verify_mandate`
   docs accordingly (no preimage byte change — the venue lists are unchanged).

**Risks:** Odra cross-contract value attachment + struct-return deserialization;
re-entrancy (preserve checks-effects-interactions — vault already records before
transfer); wasm-size growth. Mitigation: spike first; keep the direct-transfer
path behind a deploy-time venue-kind flag so a regression is bisectable.

**Green gate:** new `vault/tests/integration_adapter.rs` deploys vault +
`Cep18SwapAdapter` + composed AccessControl and proves an end-to-end atomic slice;
escrow test proves attestation-gated `record_fill`; existing guardrail tests still
pass; `ExecutionVault.wasm` within Casper install limits.

---

## Wave 3 — Guardian desk-wide pause (cross-contract wiring)

**Built:** `guardian` crate (`global_pause`/`global_resume`, paginated fan-out, a
`VaultControl` external trait) and the vault now accepts a GUARDIAN (Wave 2a
`set_guardian`).

**Remaining:**
1. Ensure the vault's `pause`/`resume`/`status` surface matches the `VaultControl`
   trait the guardian calls (signatures + entrypoint names line up for the
   generated `VaultControlContractRef`).
2. Integration test: deploy a registry + ≥2 real `ExecutionVault`s, `set_guardian`
   each to the guardian contract, then one `global_pause` pauses all of them.
3. Bound/paginate the fan-out (already designed) and tolerate already-paused
   vaults (idempotent pause).

**Green gate:** `guardian/tests` exercises fan-out against real vaults (not just
mock vaults), proving a single call pauses the desk.

---

## Wave 4 — Factory + Registry end-to-end

**Built:** `vault-registry` (enumerable index, treasury→vaults, status) and
`vault-factory` (`create_vault` using the **record-intent** model, since Casper
has no EVM-style on-chain wasm instantiation) with a `VaultRegistration` trait.

**Remaining:**
1. **Decision gate:** confirm whether Odra 2.8.1 can instantiate a stored
   contract-package version on-chain. If yes → `create_vault` instantiates and
   registers atomically. If no (likely) → keep the record-intent + emit-init-args
   model and have `scripts/src/deploy.ts` consume the registry entry (a real
   registry-driven deploy flow, just script-assisted).
2. Wire `factory → registry` via the `VaultRegistration` trait end-to-end.
3. `scripts/src/deploy.ts`: deploy via the factory/registry flow and write the
   resulting vault to the deployment manifest (idempotent).

**Green gate:** factory test creates+registers a vault (or records intent),
registry enumerates it and reverse-indexes by treasury; deploy script round-trips.

---

## Wave 5 — Multisig gating + Oracle aggregation

**Built:** `treasury-multisig` (M-of-N propose/approve/revoke/execute over action
hashes) and `OracleAggregator` (median-of-N over source `OracleAdapter`s, quorum +
per-source staleness drop), both unit-tested.

**Remaining:**
1. Gate a high-privilege action (e.g. `factory.create_vault` or guardian config)
   behind a `TreasuryMultisig` proposal/approval/execute.
2. Optionally let the vault cross-check `quoted_out`-implied price against
   `OracleAggregator::latest_price` in `execute_slice`, in addition to the static
   mandate band. Behind a config flag (oracle address may be unset).

**Green gate:** multisig threshold path (under/at/over quorum, revoke, no
double-execute); aggregator median + stale-drop + quorum-not-met revert; the vault
band cross-check is optional and does not break venues that run without an oracle.

---

## Wave 6 — Agent loop integration (TypeScript)

The persistence/finality/nonce/observability subsystems exist and are tested
(167 TS tests) but **`loop.ts` does not call them yet**. Exact call sites:

- **`agent/src/loop.ts`:** construct `FileStateStore(defaultStateDir())`,
  `InProcessNonceManager(await store.highWaterSeq())`, `RpcConfirmationService`,
  `InProcessMetrics`, `FileAuditLog`, `HealthServer` (start only when
  `HEALTH_PORT` is set). Resume operational state (breaker/priceHistory) from
  `store.loadSnapshot`; persist a `TrackSnapshot` after each outcome. Record
  metrics (`METRICS.*`) and `audit.record(...)` per tick. Wrap `executeOnce` in
  `nonce.withSequence(seq => …)` and journal the slice with `store.append`.
  In `finally`: `health.stop()`, `store.close()`, `audit.flush()`.
- **`agent/src/executor/index.ts`:** inject `ConfirmationService`; after
  `executeSlice`, gate on `confirmTransaction(sliceTxHash)` before proceeding;
  after the swap, confirm the deploy before `record_fill` (do not trust an
  unconfirmed swap). Journal each lifecycle step.
- **`agent/src/clients/vault.ts`:** optionally share one `RpcClient` with the
  `ConfirmationService`; add role-aware reads as needed. No breaking change.

**Note:** authoritative `state` (soldSoFar/boughtSoFar) should come from on-chain
**reconciliation** (`state/reconcile.ts::reconcileTrack`), not local disk — local
snapshots resume only operational heuristics (breaker/price history). Finality
gating slows the loop intentionally; keep it behind a config flag (optimistic
default for demo, finality-gated for production).

**Green gate:** agent `typecheck` + `test` green; new tests cover resume-from-
snapshot, fill blocked on unconfirmed swap, and nonce serialization across tracks.

---

## Cross-cutting (parallel to the waves)

- **Clippy-clean:** the workflow-generated crates emit a few `unused import`
  warnings; CI runs `cargo clippy -D warnings`, so clear them before the first
  `main` CI run is expected green.
- **CI/release:** pin `dtolnay/rust-toolchain` off `@master` to a SHA; confirm the
  Docker prod-prune resolves `@cadence/mandate` at runtime.
- **Deploy safety / E2E:** finality polling + deployment manifest land in
  `scripts` (done); add the Playwright E2E over the dashboard CreateMandate→sign
  flow and a testnet smoke once the contract surface settles.
- **Testnet:** rebuild `ExecutionVault.wasm` after Wave 2b, then run the
  sign → deploy → fund → run pipeline (needs the real cspr.trade venue/adapter
  address — the #1 thing that silently skips every slice if wrong).

---

## Dependency ordering

```
Wave 0 ─┬─> Wave 1 ──> Wave 2a ──> Wave 2b ─┬─> Wave 3 (guardian fan-out)
        │                                    ├─> Wave 4 (factory/registry)
        └─> Wave 5 (multisig, aggregator) ───┘   (aggregator independent of vault)
Wave 6 (agent TS) depends on Wave 2a role surface for vault.ts, but the
        persistence/confirm/nonce wiring is independent and may land first.
```

Waves 3, 4, 5 are mutually independent after 2b and can be parallelized.
