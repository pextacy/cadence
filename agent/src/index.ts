import { runAgent } from "./loop.js";

export { runAgent } from "./loop.js";
export * from "./types.js";
export { loadSignedMandate, toRuntimeMandate, type SignedMandateFile } from "./mandate.js";
export { loadConfig, type Config } from "./config.js";
export { validateSlice } from "./executor/guardrails.js";
export { computeMinOut, impliedSlippageBps, priceFixed, withinBand } from "./units.js";

// Run the agent when invoked directly (npm run dev / start).
const invokedDirectly =
  process.argv[1] !== undefined && import.meta.url === `file://${process.argv[1]}`;

if (invokedDirectly) {
  runAgent().catch((err) => {
    console.error(JSON.stringify({ event: "fatal", error: err instanceof Error ? err.message : String(err) }));
    process.exitCode = 1;
  });
}
