/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev/preview port comes from DASHBOARD_PORT (default 5173). strictPort is left
// off so Vite falls forward to the next free port instead of failing when the
// chosen one is busy — no more dead dev server on a port clash.
const port = Number(process.env.DASHBOARD_PORT ?? "5173") || 5173;

export default defineConfig({
  // Relative asset paths so the production build works when opened directly
  // (file://) or served from any sub-path, not just the domain root.
  base: "./",
  plugins: [react()],
  server: { port },
  preview: { port },
  // Vitest only owns the unit tests under src/. The Playwright E2E specs live in
  // e2e/ and use Playwright's own runner — keep them out of Vitest's glob.
  test: {
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["e2e/**", "node_modules/**", "dist/**"],
  },
});
