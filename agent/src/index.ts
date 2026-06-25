import "dotenv/config";
import { runAgent } from "./loop.js";
import { runPortfolio } from "./portfolio/run.js";

export { runAgent } from "./loop.js";
export { runPortfolio } from "./portfolio/run.js";
export { Portfolio } from "./portfolio/manager.js";
export { selectNext, isActionable } from "./portfolio/scheduler.js";
export { parsePortfolioManifest, loadPortfolioManifest } from "./portfolio/manifest.js";
export type { MandateTrack } from "./portfolio/types.js";
export { selectBestQuote } from "./routing/best-execution.js";
export { allowlistedQuotes, dedupeQuotes } from "./routing/venues.js";
export * from "./types.js";
export { loadSignedMandate, toRuntimeMandate, type SignedMandateFile } from "./mandate.js";
export { loadConfig, type Config } from "./config.js";
export { validateSlice } from "./executor/guardrails.js";
export { computeMinOut, impliedSlippageBps, priceFixed, withinBand, withinSlippage } from "./units.js";

// Run the agent when invoked directly (npm run dev / start). A configured
// PORTFOLIO_MANIFEST_PATH switches to managing several mandates concurrently.
const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;

if (invokedDirectly) {
  const run = process.env.PORTFOLIO_MANIFEST_PATH ? runPortfolio : runAgent;
  run().catch((err) => {
    console.error(JSON.stringify({ event: "fatal", error: err instanceof Error ? err.message : String(err) }));
    process.exitCode = 1;
  });
}
