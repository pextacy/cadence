/**
 * Historical on-chain activity for the vault, fetched from CSPR.cloud REST (via the
 * dev-server `/cspr-api` proxy that injects the auth header). CSPR.cloud streaming
 * is live-only, so this is how the dashboard shows what already happened — every
 * deploy that touched the vault, newest first.
 */

import { backendHttpBase } from "./backend.js";

export interface ActivityItem {
  action: string;
  detail: string;
  success: boolean;
  deployHash: string;
  blockHeight?: number;
  caller?: string;
  /** Raw sell amount (motes) for execute_slice actions, for aggregation. */
  sellMotes?: string;
}

export interface ActivitySummary {
  /** Total deploys touching the vault. */
  total: number;
  /** Successful execute_slice count. */
  slices: number;
  /** Total CSPR sold across successful slices, in motes. */
  soldMotes: bigint;
  funded: boolean;
}

/** Aggregate the deploy feed into headline numbers for the empty-state panels. */
export function summarizeActivity(items: ActivityItem[]): ActivitySummary {
  let slices = 0;
  let soldMotes = 0n;
  let funded = false;
  for (const it of items) {
    if (!it.success) continue;
    if (it.action === "Execute slice") {
      slices += 1;
      if (it.sellMotes) soldMotes += BigInt(it.sellMotes);
    }
    if (it.action === "Fund vault") funded = true;
  }
  return { total: items.length, slices, soldMotes, funded };
}

type RawArg = { parsed?: unknown } | unknown;
function parsed(a: RawArg): unknown {
  return a && typeof a === "object" && "parsed" in (a as object) ? (a as { parsed?: unknown }).parsed : a;
}

/** 9-decimal CSPR motes → human string. */
function cspr(motes: unknown): string {
  try {
    const v = BigInt(String(motes));
    const whole = v / 1_000_000_000n;
    const frac = (v % 1_000_000_000n).toString().padStart(9, "0").replace(/0+$/, "");
    return frac ? `${whole}.${frac}` : whole.toString();
  } catch {
    return String(motes);
  }
}

/** Infer the action + a human detail from a deploy's runtime args (the REST feed
 * exposes args, not the entry-point name, so we key off the arg shape). */
function describe(args: Record<string, unknown>): { action: string; detail: string } {
  const ep = args.entry_point;
  if (ep === "fund") return { action: "Fund vault", detail: `${cspr(args.amount)} CSPR attached` };
  if (ep === "seed_reserve") return { action: "Seed adapter reserve", detail: `${cspr(args.amount)} CSPR` };
  if ("sell_amount" in args)
    return {
      action: "Execute slice",
      detail: `sell ${cspr(args.sell_amount)} CSPR · min_out ${cspr(args.min_out)} · ${args.venue ?? ""}`,
    };
  if ("is_adapter" in args) return { action: "Set venue adapter", detail: `${args.venue} → ${args.is_adapter}` };
  if ("price" in args) return { action: "Set pool price", detail: String(args.price) };
  return { action: "Contract call", detail: "" };
}

export async function fetchActivity(packageHash: string): Promise<ActivityItem[]> {
  const pkg = packageHash.replace(/^(hash-|contract-package-)/, "");
  // No `order` param — CSPR.cloud 400s on an unsupported sort field; sort client-side.
  const res = await fetch(`${backendHttpBase()}/cspr-api/deploys?contract_package_hash=${pkg}&page_size=50`);
  if (!res.ok) throw new Error(`CSPR.cloud HTTP ${res.status}`);
  const json = (await res.json()) as {
    data?: Array<{
      args?: Record<string, RawArg>;
      error_message?: string | null;
      deploy_hash: string;
      block_height?: number;
      caller_public_key?: string;
    }>;
  };
  const items = (json.data ?? []).map((d): ActivityItem => {
    const args = Object.fromEntries(Object.entries(d.args ?? {}).map(([k, v]) => [k, parsed(v)]));
    const { action, detail } = describe(args);
    return {
      action,
      detail,
      success: !d.error_message,
      deployHash: d.deploy_hash,
      blockHeight: d.block_height,
      caller: d.caller_public_key,
      ...("sell_amount" in args ? { sellMotes: String(args.sell_amount) } : {}),
    };
  });
  return items.sort((a, b) => (b.blockHeight ?? 0) - (a.blockHeight ?? 0));
}
