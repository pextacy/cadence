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
| 2b | Route `execute_slice` through `VenueAdapter` (atomic + escrow-attestation paths) | ✅ Done (incl. optional protocol fee accrual to a distinct collector) |
| 3 | Guardian desk-wide pause fan-out (cross-contract wiring) | ✅ Done (incl. idempotent vault pause/resume) |
| 4 | `VaultFactory` + `VaultRegistry` create/register flow | ✅ Contracts done+tested; `register-vault` script written (idempotent, finality-confirmed) |
| 5 | `OracleAggregator` band cross-check + `TreasuryMultisig` | ✅ Oracle cross-check done; multisig gating wired (factory verifies M-of-N approval by action hash) |
| 6 | Wire the agent `loop.ts` to persistence/observability/nonce | ✅ Done (incl. on-chain startup reconciliation + finality-gating) |
| X | Cross-cutting: clippy-clean, CI green, E2E, testnet deploy-safety | ✅ clippy/CI green; dashboard sign-flow E2E added; `docker build` verified (image builds, container fail-fasts on missing env) |

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
  names must match the trait). The **escrow + signed-attestation path** for
  off-chain (cspr.trade) venues is now wired end-to-end: the `SettlementAdapter`
  stores the operator-attested `bought_amount` and exposes it via a `SettlementProof`
  cross-contract view; the vault links each escrow slice to its escrow id and
  credits `bought_so_far` from that **proven** amount through `record_escrow_fill`
  (rejecting the unproven agent-supplied `record_fill` for escrow slices, and
  refusing to advance ahead of the on-chain proof). `tests/integration_settlement.rs`
  proves the full flow. The adapter also has a **timeout refund** (`cancel_escrow`):
  if an off-chain swap never settles (operator offline / key lost), the escrow's
  recipient can reclaim the custodied sell asset after `REFUND_TIMEOUT_MS` (24h) —
  closing the only path by which escrowed funds could otherwise be locked in the
  adapter forever (the vault's `emergency_withdraw` sweeps only the vault's own
  balance). Refund and settlement are mutually exclusive terminal states.
  *Remaining:* optional protocol fee accrual on fill — a
  product decision (whether Cadence charges a fee, and to which collector), left out
  rather than guessed per CLAUDE.md §4.7; the agent-side call to `record_escrow_fill`
  + the settlement-operator service are deployment/ops wiring.
- **Wave 6** — `runAgent` uses the ops layer: `FileStateStore` snapshots per tick
  + resume-on-restart, `InProcessMetrics` counters, a hash-chained `FileAuditLog`,
  an opt-in `HealthServer` (`HEALTH_PORT`), and `InProcessNonceManager`
  serialisation. *Remaining:* on-chain state reconciliation on boot and
  finality-gating in the executor (both behind the existing modules).

Current footprint: 13 deployable contracts, 263 `cargo` tests, 13 wasm artifacts;
agent at 184 TS tests. All green on `main` (clippy `-D warnings` clean, `build-wasm.sh`
lowers every contract; `ExecutionVault.wasm` 374K).

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

## Wave 3 — Guardian desk-wide pause (cross-contract wiring) — DONE

`vault/tests/integration_guardian.rs` deploys a registry + two **real**
`ExecutionVault`s, funds them Active, registers them, `set_guardian`s each to the
guardian contract, and proves one `global_pause` fans out a cross-contract
`pause()` to both. A negative test proves an unwired guardian cannot pause (the
fan-out reverts), confirming the GUARDIAN-role authorization is load-bearing. The
`VaultControl` trait (`pause`/`resume`, no args) matches the vault's entrypoints.

**Resolved (robustness):** the vault's `pause`/`resume` are now **idempotent** —
only an `Active` vault transitions on `pause` (and only a `Paused` vault on
`resume`); any other status is a no-op rather than a revert, with authorization
(`assert_can_pause`) still enforced first. This aligns the vault with the
guardian's documented assumption, so a fan-out whose registry says `Active` but
whose vault has diverged to `Paused` (or gone terminal) no longer reverts the whole
sweep. `tests/lifecycle.rs` covers the unit idempotency cases and
`tests/integration_guardian.rs::fanout_survives_a_vault_already_paused_out_of_band`
proves the desk-wide sweep survives an out-of-band-paused vault.

---

## Wave 4 — Factory + Registry — DONE (contracts) / testnet-gated (script)

**Done + tested on-chain (34 tests):** `vault-factory/tests/factory.rs` deploys a
**real** `VaultRegistry` + `VaultFactory`, grants the factory the registry writer
role (`grant_writer`), and proves `create_vault` records an intent AND
cross-contract `register`s the vault — with negative tests for non-admin callers,
revoked admin, and duplicate registration. **Decision gate resolved:** Casper has
no EVM-style on-chain wasm instantiation, so the **record-intent + emit-init-args**
model is implemented (`create_vault(vault, treasury, agent, mandate_hash)` takes the
target address; a script deploys the wasm). The `VaultRegistration` trait is the
proven cross-contract seam.

**Remaining (testnet-gated — intentionally not shipped blind, per CLAUDE.md §4.7):**
a `scripts/` entrypoint that, given a deployed vault address + the registry hash,
submits the registry `register` (mirroring `fund.ts`'s
`ContractCallBuilder.byHash(...).entryPoint(...)`) and records it in the manifest.
Not written speculatively: its runtime correctness depends on Casper `Key`
encoding (contract vs package hash) that must be verified against a live node.

---

## Wave 5 — Oracle band cross-check DONE / Multisig gating is a design choice

**Oracle cross-check — DONE.** The vault now optionally cross-checks each slice's
implied price against an oracle: a treasury-only `set_oracle(oracle, pair,
max_deviation_bps)` configures it, and `execute_slice` calls the oracle
cross-contract via `OracleAdapterContractRef::latest_price(pair)` (the same
`OracleAdapter` trait `SignedPriceOracle` and `OracleAggregator` implement),
reverting `OraclePriceDeviation` when the deviation exceeds the band. Unset by
default (existing slices unaffected). `vault/tests/integration_oracle.rs` proves
pass-within-band and revert-outside-band against a mock `OracleAdapter`.

**Multisig — contract done+tested; gating is a deliberate design choice, not
shipped blind.** `treasury-multisig` (M-of-N propose/approve/revoke/execute) is
complete and unit-tested, but its `execute` *records* approval of an action hash —
it does not dispatch a call. Gating `factory.create_vault` therefore needs either
(a) the factory to cross-contract-read the multisig's approval of a computed
action hash, or (b) extending the multisig to dispatch arbitrary calls (a larger,
security-sensitive change). Which one is a product decision; left out rather than
guessed (CLAUDE.md §4.7).

**Green gate:** multisig threshold path (under/at/over quorum, revoke, no
double-execute); aggregator median + stale-drop + quorum-not-met revert; the vault
band cross-check is optional and does not break venues that run without an oracle.

---

## Wave 6 — Agent loop integration (TypeScript)

The persistence/finality/nonce/observability subsystems exist and are tested
(182 TS tests) and **`loop.ts` now wires them all**, including on-chain startup
reconciliation (`resolveStartupState`) and executor finality-gating. Call sites
as implemented:

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

**Note:** authoritative `state` (soldSoFar/boughtSoFar) comes from on-chain
**reconciliation** (`state/reconcile.ts::resolveStartupState`, which reads the
vault on chain and fails closed if the read fails while prior progress exists),
not local disk — local snapshots resume only operational heuristics
(breaker/price history). Finality gating slows the loop intentionally; it is
always on (fail-safe per CLAUDE.md §6) rather than behind an optimistic flag —
the executor never advances on an unconfirmed slice or swap.

**Green gate:** agent `typecheck` + `test` green; new tests cover resume-from-
snapshot, fill blocked on unconfirmed swap, and nonce serialization across tracks.

---

## Cross-cutting (parallel to the waves)

- **Clippy-clean:** ✅ done. `cargo clippy -D warnings` is green and the
  `build-wasm.sh` lowering is warning-free — the `errors = Error` attributes that
  went `unused` under the wasm build cfg now reference the error type by full path
  (no standalone `use`), so no crate emits an unused-import warning.
- **CI/release:** ✅ `dtolnay/rust-toolchain` is pinned to a SHA (not `@master`).
  The Docker prod-prune resolves `@cadence/mandate` via the workspace symlink
  (runtime copies `mandate/dist` + manifest); full verification still wants a real
  `docker build`.
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
