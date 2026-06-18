import { useEffect, useMemo, useRef, useState } from "react";
import type { DashboardConfig } from "../config.js";
import { isPortfolioConfigured, isStreamConfigured } from "../config.js";
import type { DashboardState, VaultEvent } from "../types.js";
import { initialState, reduceEvent } from "./events.js";

export type ConnectionStatus = "idle" | "connecting" | "open" | "error" | "closed";

export interface StreamResult {
  state: DashboardState;
  connection: ConnectionStatus;
  /** Number of vault events applied so far. */
  applied: number;
  lastError?: string;
}

/**
 * Map a raw CSPR.cloud streaming message to a typed {@link VaultEvent}. The vault
 * emits CES events whose name and fields appear in the message payload. Anything
 * that does not match a known vault event is ignored — never invented.
 */
export function mapStreamMessage(msg: unknown): VaultEvent | null {
  if (typeof msg !== "object" || msg === null) return null;
  const m = msg as Record<string, unknown>;
  const data = (m.data ?? m) as Record<string, unknown>;
  const name = (data.name ?? data.event_name ?? data.kind) as string | undefined;
  if (!name) return null;
  const f = (data.data ?? data.fields ?? data) as Record<string, unknown>;
  const str = (k: string): string => String(f[k] ?? "");
  const num = (k: string): number => Number(f[k] ?? 0);
  const bool = (k: string): boolean => Boolean(f[k]);
  const tsRaw = m.timestamp ?? data.timestamp;
  const atMs = tsRaw ? Date.parse(String(tsRaw)) : undefined;

  switch (name) {
    case "MandateInitialised":
      return { kind: "MandateInitialised", treasury: str("treasury"), agent: str("agent"), sellAsset: str("sell_asset"), buyAsset: str("buy_asset"), totalSell: str("total_sell"), endTimeMs: num("end_time_ms"), maxSlippageBps: num("max_slippage_bps") };
    case "VaultFunded":
      return { kind: "VaultFunded", amount: str("amount"), balance: str("balance") };
    case "SliceExecuted":
      return { kind: "SliceExecuted", sliceId: num("slice_id"), sellAmount: str("sell_amount"), quotedOut: str("quoted_out"), minOut: str("min_out"), venue: str("venue"), soldSoFar: str("sold_so_far"), ...(f.deploy_hash ? { deployHash: str("deploy_hash") } : {}), ...(atMs !== undefined && !Number.isNaN(atMs) ? { atMs } : {}) };
    case "FillRecorded":
      return { kind: "FillRecorded", sliceId: num("slice_id"), boughtAmount: str("bought_amount"), swapDeployHash: str("swap_deploy_hash"), boughtSoFar: str("bought_so_far") };
    case "DecisionAttested":
      return { kind: "DecisionAttested", sliceId: num("slice_id"), reason: str("reason") };
    case "StatusChanged":
      return { kind: "StatusChanged", paused: bool("paused") };
    case "Settled":
      return { kind: "Settled", completed: bool("completed"), soldSoFar: str("sold_so_far"), boughtSoFar: str("bought_so_far"), sliceCount: num("slice_count"), returnedToTreasury: str("returned_to_treasury") };
    default:
      return null;
  }
}

/**
 * Subscribe to the vault's on-chain events over CSPR.cloud streaming and
 * reconstruct dashboard state. When streaming is unconfigured it stays `idle`
 * with empty state — the screens render an honest "not connected" message.
 */
export function useVaultStream(cfg: DashboardConfig): StreamResult {
  const [state, setState] = useState<DashboardState>(initialState);
  const [connection, setConnection] = useState<ConnectionStatus>("idle");
  const [applied, setApplied] = useState(0);
  const [lastError, setLastError] = useState<string | undefined>(undefined);
  const seen = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (!isStreamConfigured(cfg)) {
      setConnection("idle");
      return;
    }
    setConnection("connecting");
    const channel = `/contract-events?contract_hash=${cfg.vaultContractHash}`;
    const ws = new WebSocket(`${cfg.streamingUrl}${channel}`);

    ws.addEventListener("open", () => {
      setConnection("open");
      ws.send(JSON.stringify({ action: "subscribe", token: cfg.apiKey, contract_hash: cfg.vaultContractHash }));
    });
    ws.addEventListener("message", (ev) => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(typeof ev.data === "string" ? ev.data : String(ev.data));
      } catch {
        return;
      }
      // Deduplicate by a stable id when present.
      const env = parsed as Record<string, unknown>;
      const id = (env.id ?? env.event_id) as string | undefined;
      if (id) {
        if (seen.current.has(id)) return;
        seen.current.add(id);
      }
      const vaultEvent = mapStreamMessage(parsed);
      if (!vaultEvent) return;
      setState((prev) => reduceEvent(prev, vaultEvent));
      setApplied((n) => n + 1);
    });
    ws.addEventListener("error", () => {
      setConnection("error");
      setLastError("Streaming connection error");
    });
    ws.addEventListener("close", () => setConnection((c) => (c === "error" ? c : "closed")));

    return () => ws.close();
  }, [cfg]);

  return { state, connection, applied, lastError };
}

/** One vault's reconstructed state and connection, within a portfolio stream. */
export interface VaultStreamView {
  id: string;
  state: DashboardState;
  connection: ConnectionStatus;
}

export interface PortfolioStreamResult {
  vaults: VaultStreamView[];
  /** Aggregate connection: open if any vault is open, else the most active state. */
  connection: ConnectionStatus;
}

/** Reduce per-vault connection statuses to one headline status. */
function aggregateConnection(views: readonly VaultStreamView[]): ConnectionStatus {
  if (views.length === 0) return "idle";
  const states = views.map((v) => v.connection);
  if (states.includes("open")) return "open";
  if (states.includes("connecting")) return "connecting";
  if (states.includes("error")) return "error";
  if (states.every((s) => s === "idle")) return "idle";
  return "closed";
}

/**
 * Subscribe to several vaults at once (one WebSocket per vault) and reconstruct
 * each one's state independently. Used by the portfolio screen. When streaming is
 * unconfigured every vault stays idle with empty state — honest, never sampled.
 */
export function usePortfolioStream(cfg: DashboardConfig): PortfolioStreamResult {
  // Stable key so the effect only re-subscribes when the actual vault set changes.
  const hashKey = cfg.vaultContractHashes.join(",");
  const [byId, setById] = useState<Record<string, VaultStreamView>>({});
  const seen = useRef<Set<string>>(new Set());

  useEffect(() => {
    const hashes = cfg.vaultContractHashes;
    const fresh: Record<string, VaultStreamView> = Object.fromEntries(
      hashes.map((h) => [h, { id: h, state: initialState(), connection: "idle" as ConnectionStatus }]),
    );
    setById(fresh);
    seen.current = new Set();

    if (!isPortfolioConfigured(cfg)) return;

    const setConn = (id: string, connection: ConnectionStatus): void =>
      setById((prev) => (prev[id] ? { ...prev, [id]: { ...prev[id]!, connection } } : prev));

    const sockets = hashes.map((id) => {
      setConn(id, "connecting");
      const ws = new WebSocket(`${cfg.streamingUrl}/contract-events?contract_hash=${id}`);
      ws.addEventListener("open", () => {
        setConn(id, "open");
        ws.send(JSON.stringify({ action: "subscribe", token: cfg.apiKey, contract_hash: id }));
      });
      ws.addEventListener("message", (ev) => {
        let parsed: unknown;
        try {
          parsed = JSON.parse(typeof ev.data === "string" ? ev.data : String(ev.data));
        } catch {
          return;
        }
        const env = parsed as Record<string, unknown>;
        const eid = (env.id ?? env.event_id) as string | undefined;
        if (eid) {
          const key = `${id}:${eid}`;
          if (seen.current.has(key)) return;
          seen.current.add(key);
        }
        const vaultEvent = mapStreamMessage(parsed);
        if (!vaultEvent) return;
        setById((prev) =>
          prev[id] ? { ...prev, [id]: { ...prev[id]!, state: reduceEvent(prev[id]!.state, vaultEvent) } } : prev,
        );
      });
      ws.addEventListener("error", () => setConn(id, "error"));
      ws.addEventListener("close", () =>
        setById((prev) =>
          prev[id] && prev[id]!.connection !== "error"
            ? { ...prev, [id]: { ...prev[id]!, connection: "closed" } }
            : prev,
        ),
      );
      return ws;
    });

    return () => sockets.forEach((ws) => ws.close());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cfg, hashKey]);

  const vaults = useMemo(
    () => cfg.vaultContractHashes.map((h) => byId[h] ?? { id: h, state: initialState(), connection: "idle" as ConnectionStatus }),
    [cfg.vaultContractHashes, byId],
  );

  return { vaults, connection: aggregateConnection(vaults) };
}
