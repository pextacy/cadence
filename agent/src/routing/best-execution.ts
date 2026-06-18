import type { Quote } from "../types.js";
import { allowlistedQuotes, dedupeQuotes } from "./venues.js";

/**
 * Best-execution selection: among allowlisted quotes for the *same* sell amount,
 * pick the one with the highest output (best price). Deterministic — ties break
 * by venue name then venue address, so the choice is reproducible. Returns null
 * when no allowlisted quote has a positive output.
 *
 * Pure: the executor still re-validates the chosen quote against every guardrail,
 * and the vault re-validates again on-chain.
 */
export function selectBestQuote(
  quotes: readonly Quote[],
  allowlist: readonly string[],
): Quote | null {
  const eligible = dedupeQuotes(allowlistedQuotes(quotes, allowlist)).filter(
    (q) => q.quotedOut > 0n,
  );
  if (eligible.length === 0) return null;
  return eligible.reduce((best, q) => {
    if (q.quotedOut !== best.quotedOut) return q.quotedOut > best.quotedOut ? q : best;
    if (q.venue !== best.venue) return q.venue < best.venue ? q : best;
    return q.venueAddress < best.venueAddress ? q : best;
  });
}
