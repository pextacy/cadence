import Anthropic from "@anthropic-ai/sdk";
import type { MarketSnapshot, RuntimeMandate, SliceProposal, VaultState } from "../types.js";
import { parseProposal } from "./schema.js";
import { evenSplit } from "./strategies/twap.js";
import { strategyFor } from "./strategies/registry.js";

export interface PlannerInput {
  mandate: RuntimeMandate;
  state: VaultState;
  market: MarketSnapshot;
  nowMs: number;
  /** Target number of slices across the window (TWAP granularity). */
  targetSlices: number;
}

/**
 * Deterministic TWAP reference size: split the remaining size evenly across the
 * remaining slices. Pure — kept as a stable reference; delegates to the shared
 * {@link evenSplit} primitive the TWAP strategy is built on. Never the authority:
 * the executor validates the planner's output.
 */
export function twapBaseline(input: {
  remaining: bigint;
  slicesRemaining: number;
}): bigint {
  return evenSplit(input.remaining, input.slicesRemaining);
}

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

/** LLM planner. Produces an untrusted proposal; the executor validates it. */
export class Planner {
  private readonly client: Anthropic;

  constructor(
    apiKey: string,
    private readonly model: string,
  ) {
    this.client = new Anthropic({ apiKey });
  }

  async propose(input: PlannerInput): Promise<SliceProposal> {
    const remaining = input.mandate.totalSell - input.state.soldSoFar;
    const slicesRemaining = Math.max(1, input.targetSlices - input.state.sliceCount);
    const { sliceSize: reference, suggestedSlippageBps } = strategyFor(input.mandate.strategy)({
      remaining,
      slicesRemaining,
      mandate: input.mandate,
      market: input.market,
    });

    const userMessage = JSON.stringify({
      mandate: {
        totalSell: input.mandate.totalSell.toString(),
        endTimeMs: input.mandate.endTimeMs,
        maxSlippageBps: input.mandate.maxSlippageBps,
        priceFloor: input.mandate.priceFloor.toString(),
        priceCeiling: input.mandate.priceCeiling.toString(),
        strategy: input.mandate.strategy,
        venueAllowlist: input.mandate.venueAllowlist,
      },
      state: {
        soldSoFar: input.state.soldSoFar.toString(),
        remaining: remaining.toString(),
        sliceCount: input.state.sliceCount,
      },
      market: {
        midPrice: input.market.midPrice.toString(),
        volatilityBps: input.market.volatilityBps ?? null,
        depthSell: input.market.depthSell?.toString() ?? null,
      },
      nowMs: input.nowMs,
      referenceSliceSize: reference.toString(),
      suggestedSlippageBps,
      slicesRemaining,
    });

    const res = await this.client.messages.create({
      model: this.model,
      max_tokens: 512,
      system: SYSTEM_PROMPT,
      messages: [{ role: "user", content: userMessage }],
    });

    const text = res.content
      .filter((b): b is Anthropic.TextBlock => b.type === "text")
      .map((b) => b.text)
      .join("")
      .trim();

    return parseProposal(extractJson(text));
  }
}

/** Extract the first JSON object from a model response, tolerating code fences. */
export function extractJson(text: string): unknown {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/);
  const candidate = fenced?.[1] ?? text;
  const start = candidate.indexOf("{");
  const end = candidate.lastIndexOf("}");
  if (start === -1 || end === -1 || end < start) {
    throw new Error("Planner response contained no JSON object");
  }
  return JSON.parse(candidate.slice(start, end + 1));
}
