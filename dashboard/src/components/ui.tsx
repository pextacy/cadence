import type { ReactNode } from "react";
import type { VaultStatus } from "../types.js";

const STATUS_CLASS: Record<VaultStatus | "Unknown", string> = {
  Funded: "wait",
  Active: "ok",
  Paused: "hold",
  Completed: "ok",
  Expired: "stop",
  Halted: "stop",
  Unknown: "wait",
};

export function StatusBadge({ status }: { status: VaultStatus | "Unknown" }): JSX.Element {
  return (
    <span className={`badge ${STATUS_CLASS[status]}`}>
      <span className="dot" />
      {status}
    </span>
  );
}

const SLICE_CLASS = { filled: "ok", blocked: "stop", pending: "wait" } as const;
const SLICE_LABEL = { filled: "Filled", blocked: "Blocked", pending: "Pending" } as const;

export function SliceBadge({ status }: { status: "pending" | "filled" | "blocked" }): JSX.Element {
  return (
    <span className={`badge ${SLICE_CLASS[status]}`}>
      <span className="dot" />
      {SLICE_LABEL[status]}
    </span>
  );
}

/** A single headline figure. A null/undefined value renders an honest dash. */
export function Stat({
  label,
  value,
  unit,
}: {
  label: string;
  value: string | null | undefined;
  unit?: string;
}): JSX.Element {
  const has = value !== null && value !== undefined;
  return (
    <div className="stat">
      <div className="label">{label}</div>
      <div className={`value${has ? "" : " muted"}`}>
        {has ? value : "—"}
        {has && unit ? <span className="unit">{unit}</span> : null}
      </div>
    </div>
  );
}

export function NonDataState({
  kind,
  title,
  children,
}: {
  kind: "loading" | "empty" | "error";
  title: string;
  children?: ReactNode;
}): JSX.Element {
  return (
    <div className={`state${kind === "error" ? " error" : ""}`}>
      <div className="big">{title}</div>
      {children}
    </div>
  );
}
