import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// Dev/preview port comes from DASHBOARD_PORT (default 5173). strictPort is left
// off so Vite falls forward to the next free port instead of failing when the
// chosen one is busy — no more dead dev server on a port clash.
const port = Number(process.env.DASHBOARD_PORT ?? "5173") || 5173;

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const streamHost = (env.VITE_CSPR_CLOUD_STREAMING_URL || "wss://streaming.testnet.cspr.cloud").trim();
  const apiKey = (env.VITE_CSPR_CLOUD_API_KEY || "").trim();
  // LLM_API_KEY is NOT a VITE_ var, so it stays server-side here (never bundled to
  // the browser). The /gemini proxy appends it so the dashboard can show live
  // planner reasoning without exposing the key.
  const geminiKey = (env.LLM_API_KEY || "").trim();

  return {
    // Relative asset paths so the production build works when opened directly
    // (file://) or served from any sub-path, not just the domain root.
    base: "./",
    plugins: [react()],
    server: {
      port,
      // CSPR.cloud streaming requires the API token in an `authorization` HTTP
      // header, which a browser WebSocket cannot set. Proxy a same-origin path
      // through the dev server and inject the header on the WS upgrade request.
      proxy: {
        "/cspr-stream": {
          target: streamHost,
          ws: true,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/cspr-stream/, ""),
          configure: (proxy) => {
            proxy.on("proxyReqWs", (proxyReq) => {
              if (apiKey) proxyReq.setHeader("authorization", apiKey);
              // CSPR.cloud 403s a browser `Origin` header; strip it so the upgrade
              // looks server-to-server (the dev proxy is the real client).
              proxyReq.removeHeader("origin");
            });
          },
        },
        // CSPR.cloud REST — same header-auth issue as streaming; proxy it and inject
        // the token so the dashboard can fetch historical on-chain activity (deploys).
        "/cspr-api": {
          target: (env.VITE_CSPR_CLOUD_REST_URL || "https://api.testnet.cspr.cloud").trim(),
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/cspr-api/, ""),
          configure: (proxy) => {
            proxy.on("proxyReq", (proxyReq) => {
              if (apiKey) proxyReq.setHeader("authorization", apiKey);
            });
          },
        },
        // Google Gemini — the dashboard POSTs to /gemini/models/<model>:generateContent;
        // the proxy appends the server-side key so live LLM reasoning works without
        // shipping the key to the browser.
        "/gemini": {
          target: "https://generativelanguage.googleapis.com",
          changeOrigin: true,
          rewrite: (p) => {
            const base = p.replace(/^\/gemini/, "/v1beta");
            return base + (base.includes("?") ? "&" : "?") + "key=" + geminiKey;
          },
        },
      },
    },
    preview: { port },
  };
});
