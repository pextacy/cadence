import type { Quote } from "../types.js";

/**
 * Keep only quotes whose venue is on the mandate allowlist. The vault enforces
 * the same allowlist on-chain in `execute_slice`; filtering here avoids quoting a
 * venue the contract would reject. Pure.
 */
export function allowlistedQuotes(
  quotes: readonly Quote[],
  allowlist: readonly string[],
): Quote[] {
  return quotes.filter((q) => allowlist.includes(q.venue));
}

/**
 * Deduplicate quotes that point at the same venue address (the routing backend
 * may return the same pool for several venue hints). Keeps the first occurrence;
 * order preserved. Pure.
 */
export function dedupeQuotes(quotes: readonly Quote[]): Quote[] {
  const seen = new Set<string>();
  const out: Quote[] = [];
  for (const q of quotes) {
    const key = `${q.venue}:${q.venueAddress}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(q);
  }
  return out;
}
