/**
 * Production server for the Cadence dashboard.
 *
 * The dashboard's live features (CSPR.cloud streaming, CSPR.cloud REST, the Gemini
 * planner) all need a server-side proxy: the browser cannot set the `authorization`
 * header on a WebSocket, CSPR.cloud 403s a browser `Origin`, and the API keys must
 * never reach the browser. In dev this is done by Vite's dev-server proxy
 * (vite.config.ts); this file reproduces exactly the same three proxies for a
 * production host that supports WebSockets (e.g. Render). Vercel cannot host this —
 * its serverless functions can't proxy a persistent WebSocket.
 *
 * Env (all server-side, NOT VITE_ — so never bundled to the browser):
 *   CSPR_CLOUD_API_KEY        CSPR.cloud REST + streaming token
 *   LLM_API_KEY               Google Gemini key
 *   CSPR_CLOUD_STREAMING_URL  optional override (default testnet)
 *   CSPR_CLOUD_REST_URL       optional override (default testnet)
 *   PORT                      injected by the host
 */
import express from "express";
import rateLimit from "express-rate-limit";
import { createProxyMiddleware } from "http-proxy-middleware";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const distDir = path.join(__dirname, "dist");
const port = Number(process.env.PORT) || 8080;

const streamHost = (process.env.CSPR_CLOUD_STREAMING_URL || "wss://streaming.testnet.cspr.cloud").trim();
const restHost = (process.env.CSPR_CLOUD_REST_URL || "https://api.testnet.cspr.cloud").trim();
const apiKey = (process.env.CSPR_CLOUD_API_KEY || "").trim();
const geminiKey = (process.env.LLM_API_KEY || "").trim();
// Split deploy (frontend on another origin, e.g. Vercel): allow that origin to
// call the proxy routes cross-origin. Default "*" — the proxied data is public
// on-chain state and the secret keys stay server-side. Set ALLOW_ORIGIN to the
// exact Vercel URL to lock it down.
const allowOrigin = (process.env.ALLOW_ORIGIN || "*").trim();

const app = express();

// Rate-limit every HTTP route (proxies + static file serving) to cap abuse/DoS.
// Generous for a live demo with several concurrent viewers; the WebSocket upgrade
// bypasses express (server.on('upgrade')) so live streaming is unaffected.
app.use(
  rateLimit({
    windowMs: 60_000,
    limit: 600,
    standardHeaders: "draft-7",
    legacyHeaders: false,
  }),
);

// CORS for the proxy routes only. WebSocket upgrades don't use CORS; browser
// fetch (activity, gemini) does — including a preflight for the gemini POST.
app.use((req, res, next) => {
  if (/^\/(cspr-api|gemini|cspr-stream)/.test(req.path)) {
    res.setHeader("Access-Control-Allow-Origin", allowOrigin);
    res.setHeader("Vary", "Origin");
    res.setHeader("Access-Control-Allow-Headers", "content-type, authorization");
    res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    if (req.method === "OPTIONS") return res.sendStatus(204);
  }
  next();
});

// CSPR.cloud REST (`/cspr-api/*`) — inject the auth header; the browser never
// sees the key. Mirrors the `/cspr-api` proxy in vite.config.ts.
app.use(
  "/cspr-api",
  createProxyMiddleware({
    target: restHost,
    changeOrigin: true,
    pathRewrite: { "^/cspr-api": "" },
    on: {
      proxyReq: (proxyReq) => {
        if (apiKey) proxyReq.setHeader("authorization", apiKey);
      },
      proxyRes: (proxyRes) => {
        proxyRes.headers["access-control-allow-origin"] = allowOrigin;
      },
    },
  }),
);

// Google Gemini (`/gemini/*`) — append the server-side key as a query param.
// Mirrors the `/gemini` proxy in vite.config.ts.
app.use(
  "/gemini",
  createProxyMiddleware({
    target: "https://generativelanguage.googleapis.com",
    changeOrigin: true,
    pathRewrite: (p) => {
      // express has already stripped the "/gemini" mount prefix, so `p` is like
      // "/models/xxx:generateContent". Prepend the API version and append the key.
      const base = "/v1beta" + p;
      return base + (base.includes("?") ? "&" : "?") + "key=" + geminiKey;
    },
    on: {
      proxyRes: (proxyRes) => {
        proxyRes.headers["access-control-allow-origin"] = allowOrigin;
      },
    },
  }),
);

// CSPR.cloud streaming (`/cspr-stream/*`) — WebSocket proxy. Inject the auth
// header and strip the browser `Origin` (CSPR.cloud 403s it on the WS upgrade).
// Mirrors the `/cspr-stream` proxy in vite.config.ts.
const streamProxy = createProxyMiddleware({
  target: streamHost,
  changeOrigin: true,
  ws: true,
  pathRewrite: { "^/cspr-stream": "" },
  on: {
    proxyReqWs: (proxyReq) => {
      if (apiKey) proxyReq.setHeader("authorization", apiKey);
      proxyReq.removeHeader("origin");
    },
  },
});
app.use("/cspr-stream", streamProxy);

// Static build. The app uses HashRouter, so every route lives under `/#/…` and
// the server only ever serves `/`; the catch-all is a harmless safety net.
app.use(express.static(distDir));
app.get("*", (_req, res) => res.sendFile(path.join(distDir, "index.html")));

const server = app.listen(port, () => {
  console.log(`Cadence dashboard listening on :${port}`);
  if (!apiKey) console.warn("WARN: CSPR_CLOUD_API_KEY is not set — live streaming/REST will not authenticate.");
  if (!geminiKey) console.warn("WARN: LLM_API_KEY is not set — the AI planner will not work.");
});

// Route WebSocket upgrades. Only `/cspr-stream` is proxied; anything else is
// rejected rather than left hanging.
server.on("upgrade", (req, socket, head) => {
  if (req.url && req.url.startsWith("/cspr-stream")) {
    streamProxy.upgrade(req, socket, head);
  } else {
    socket.destroy();
  }
});
