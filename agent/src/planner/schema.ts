import { z } from "zod";
import type { SliceProposal } from "../types.js";

/**
 * The planner LLM must return JSON matching this schema and nothing else. The
 * executor treats the output as untrusted: it parses with this schema and then
 * re-validates every field against the mandate guardrails before acting.
 */
export const ProposalSchema = z.object({
  /** Decimal string of the next child-order size in sell-asset base units. */
  sellAmount: z
    .string()
    .regex(/^\d+$/, "sellAmount must be a non-negative integer string"),
  /** Earliest submission time, unix milliseconds. */
  notBeforeMs: z.number().int().nonnegative(),
  /** Per-slice slippage cap in bps; clamped to the mandate cap by the executor. */
  maxSlippageBps: z.number().int().min(0).max(10_000),
  /** A short human-readable reason recorded as the on-chain attestation. */
  reason: z.string().min(1).max(280),
});

export type RawProposal = z.infer<typeof ProposalSchema>;

/** Parse and normalise raw planner JSON into a typed {@link SliceProposal}. */
export function parseProposal(raw: unknown): SliceProposal {
  const p = ProposalSchema.parse(raw);
  return {
    sellAmount: BigInt(p.sellAmount),
    notBeforeMs: p.notBeforeMs,
    maxSlippageBps: p.maxSlippageBps,
    reason: p.reason,
  };
}
