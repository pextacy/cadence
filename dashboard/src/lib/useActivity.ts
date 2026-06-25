import { useEffect, useState } from "react";
import { fetchActivity, summarizeActivity, type ActivityItem, type ActivitySummary } from "./activity.js";

/**
 * Fetch the vault's on-chain deploy history once (and aggregate it) so screens can
 * show real numbers even when the live event stream is empty. CSPR.cloud streaming
 * only carries new events; this fills the gap with what already happened on-chain.
 */
export function useActivity(packageHash?: string): {
  items: ActivityItem[] | null;
  summary: ActivitySummary | null;
  error: string | null;
} {
  const [items, setItems] = useState<ActivityItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!packageHash) return;
    let cancelled = false;
    fetchActivity(packageHash)
      .then((i) => !cancelled && setItems(i))
      .catch((e) => !cancelled && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      cancelled = true;
    };
  }, [packageHash]);

  return { items, summary: items ? summarizeActivity(items) : null, error };
}
