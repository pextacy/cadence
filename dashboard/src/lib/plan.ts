/**
 * Live LLM (Google Gemini) planner commentary for the dashboard. Calls the same
 * model + prompt the agent's planner uses, through the dev-server `/gemini` proxy
 * so the API key is never shipped to the browser. Returns the proposed next slice
 * and the model's one-sentence reasoning — the "what would the agent do now, and
 * why" view.
 */

import { backendHttpBase } from "./backend.js";

const MODEL = "gemini-2.5-flash";

const SYSTEM_PROMPT = `You are the planning module of Cadence, an autonomous OTC execution desk.
Your only job is to propose the NEXT child order (slice) following the mandate's execution
strategy. You never hold keys and never execute trades; a deterministic executor and an
on-chain contract validate and enforce every limit you propose. You cannot breach a limit.

Return ONLY a single JSON object, no prose, with exactly these fields:
{
  "sellAmount": string,      // integer, sell-asset base units, > 0
  "notBeforeMs": number,     // unix ms, when this slice may be submitted
  "maxSlippageBps": number,  // <= suggestedSlippageBps
  "reason": string           // one concise sentence (<= 280 chars)
}
Constraints you must respect:
- sellAmount + soldSoFar must not exceed totalSell.
- referenceSliceSize is the active strategy's suggested size; stay close to it unless
  little time remains, and never dump the whole remainder at once early in the window.
- Keep maxSlippageBps at or below suggestedSlippageBps.
- If volatility is elevated, propose a smaller slice and tighten maxSlippageBps further.`;

export interface PlanInput {
  sellAsset: string;
  buyAsset: string;
  totalSell: bigint;
  soldSoFar: bigint;
  maxSlippageBps: number;
  strategy: string;
  /** Remaining slices to spread over (TWAP granularity). */
  slicesRemaining: number;
  /** Optional mid-price (fixed-point) and volatility for context. */
  midPrice?: bigint;
  volatilityBps?: number;
}

export interface PlanResult {
  sellAmount: bigint;
  maxSlippageBps: number;
  reason: string;
  /** Raw model text, for the "show the model output" detail. */
  raw: string;
}

/** Extract the first JSON object from model text, tolerating code fences. */
function extractJson(text: string): unknown {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/);
  const candidate = fenced?.[1] ?? text;
  const start = candidate.indexOf("{");
  const end = candidate.lastIndexOf("}");
  if (start === -1 || end === -1 || end < start) {
    throw new Error("Planner response contained no JSON object");
  }
  return JSON.parse(candidate.slice(start, end + 1));
}

export async function askPlanner(input: PlanInput): Promise<PlanResult> {
  const remaining = input.totalSell > input.soldSoFar ? input.totalSell - input.soldSoFar : 0n;
  const reference = remaining / BigInt(Math.max(1, input.slicesRemaining));
  const userMessage = JSON.stringify({
    mandate: {
      totalSell: input.totalSell.toString(),
      maxSlippageBps: input.maxSlippageBps,
      strategy: input.strategy,
      sellAsset: input.sellAsset,
      buyAsset: input.buyAsset,
    },
    state: { soldSoFar: input.soldSoFar.toString(), remaining: remaining.toString() },
    market: {
      midPrice: input.midPrice?.toString() ?? null,
      volatilityBps: input.volatilityBps ?? null,
    },
    nowMs: Date.now(),
    referenceSliceSize: reference.toString(),
    suggestedSlippageBps: input.maxSlippageBps,
    slicesRemaining: input.slicesRemaining,
  });

  const res = await fetch(`${backendHttpBase()}/gemini/models/${MODEL}:generateContent`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      system_instruction: { parts: [{ text: SYSTEM_PROMPT }] },
      contents: [{ role: "user", parts: [{ text: userMessage }] }],
      generationConfig: {
        responseMimeType: "application/json",
        maxOutputTokens: 1024,
        temperature: 0,
        thinkingConfig: { thinkingBudget: 0 },
      },
    }),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`Gemini HTTP ${res.status}${body ? `: ${body.slice(0, 200)}` : ""}`);
  }
  const data = (await res.json()) as {
    candidates?: Array<{ content?: { parts?: Array<{ text?: string }> } }>;
  };
  const text = (data.candidates?.[0]?.content?.parts ?? [])
    .map((p) => p.text ?? "")
    .join("")
    .trim();
  if (!text) throw new Error("Gemini returned an empty response");
  const obj = extractJson(text) as { sellAmount?: string; maxSlippageBps?: number; reason?: string };
  return {
    sellAmount: BigInt(obj.sellAmount ?? "0"),
    maxSlippageBps: Number(obj.maxSlippageBps ?? 0),
    reason: obj.reason ?? "",
    raw: text,
  };
}
