/**
 * Canned CSPR.cloud streaming messages for the vault, in the EXACT envelope shape
 * `mapStreamMessage` (dashboard/src/lib/useVaultStream.ts) expects.
 *
 * `mapStreamMessage` resolves the event name from `data.name` where
 * `data = msg.data ?? msg`, and the field bag from `f = data.data ?? data.fields ?? data`.
 *
 * Because `data` falls back to `msg` only when `msg.data` is absent, the safe,
 * unambiguous envelope is FLAT: name, id, timestamp and every event field live at
 * the top level. Then `data = msg`, `name = msg.name`, and `f = msg` — so the
 * snake_case fields (`sell_asset`, `slice_id`, …) are read directly. A nested
 * `{ name, data: {...} }` would shadow `name` (data would become the inner bag),
 * which is exactly why this fixture keeps everything flat.
 *
 * Each message carries a unique `id` so the hook's dedupe set never drops one, and
 * an ISO `timestamp` so SliceExecuted gets an `atMs`.
 */
export type StreamMessage = Record<string, unknown> & {
  id: string;
  name: string;
  timestamp: string;
};

let seq = 0;
function msg(name: string, fields: Record<string, unknown>): StreamMessage {
  seq += 1;
  return {
    id: `evt-${seq}`,
    name,
    timestamp: new Date(1_700_000_000_000 + seq * 60_000).toISOString(),
    ...fields,
  };
}

const TREASURY = "0x1111111111111111111111111111111111111111";
const AGENT = "0x2222222222222222222222222222222222222222";

// A far-future end time keeps "Time left" positive and deterministic-ish.
const END_TIME_MS = 4_102_444_800_000; // 2100-01-01

/**
 * Realistic lifecycle WITHOUT settlement — for the LiveExecution screen.
 * Mandate funded & active, one slice executed and filled, decision attested.
 */
export function liveSequence(): StreamMessage[] {
  return [
    msg("MandateInitialised", {
      treasury: TREASURY,
      agent: AGENT,
      sell_asset: "CSPR",
      buy_asset: "USDC",
      total_sell: "2000000000000", // 2,000,000 CSPR (6 decimals)
      end_time_ms: END_TIME_MS,
      max_slippage_bps: 100,
    }),
    msg("VaultFunded", { amount: "2000000000000", balance: "2000000000000" }),
    msg("SliceExecuted", {
      slice_id: 0,
      sell_amount: "500000000000",
      quoted_out: "510000000",
      min_out: "504900000",
      venue: "cspr.trade",
      sold_so_far: "500000000000",
      deploy_hash: "0xabc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abcd",
    }),
    msg("DecisionAttested", { slice_id: 0, reason: "TWAP slice 1/4 within slippage band" }),
    msg("FillRecorded", {
      slice_id: 0,
      bought_amount: "509000000",
      swap_deploy_hash: "0xdef456def456def456def456def456def456def456def456def456def456def4",
      bought_so_far: "509000000",
    }),
  ];
}

/**
 * Full lifecycle ENDING in a completed Settled event — for the FinalReport screen.
 */
export function settledSequence(): StreamMessage[] {
  return [
    ...liveSequence(),
    msg("SliceExecuted", {
      slice_id: 1,
      sell_amount: "1500000000000",
      quoted_out: "1530000000",
      min_out: "1514700000",
      venue: "cspr.trade",
      sold_so_far: "2000000000000",
      deploy_hash: "0x111122223333444455556666777788889999aaaabbbbccccddddeeeeffff0000",
    }),
    msg("FillRecorded", {
      slice_id: 1,
      bought_amount: "1531000000",
      swap_deploy_hash: "0x0000ffffeeeeddddccccbbbbaaaa99998888777766665555444433332222111",
      bought_so_far: "2040000000",
    }),
    msg("Settled", {
      completed: true,
      sold_so_far: "2000000000000",
      bought_so_far: "2040000000",
      slice_count: 2,
      returned_to_treasury: "0",
    }),
  ];
}
