/**
 * Where the dashboard reaches its proxy backend (CSPR.cloud REST + streaming +
 * Gemini). Two topologies are supported by one env var:
 *
 *   VITE_BACKEND_ORIGIN unset  → same-origin. The page and the proxies are served
 *     by one host (local `vite dev`, or the all-in-one Render deploy running
 *     dashboard/server.mjs). Fetches are relative ("/cspr-api"), the WebSocket
 *     uses the page's own host.
 *
 *   VITE_BACKEND_ORIGIN set    → split deploy. The static frontend (e.g. Vercel)
 *     calls a separate backend (e.g. Render running server.mjs) at this absolute
 *     origin. Set it to the backend's https URL, no trailing slash.
 */
function origin(): string {
  const o = import.meta.env.VITE_BACKEND_ORIGIN as string | undefined;
  return typeof o === "string" && o ? o.replace(/\/+$/, "") : "";
}

/** Base for REST/HTTP proxy calls. "" means same-origin relative paths. */
export function backendHttpBase(): string {
  return origin();
}

/** Base (scheme + host) for the streaming WebSocket. */
export function backendWsBase(): string {
  const o = origin();
  if (o) return o.replace(/^http/, "ws"); // https→wss, http→ws
  const proto = window.location.protocol === "https:" ? "wss" : "ws";
  return `${proto}://${window.location.host}`;
}
