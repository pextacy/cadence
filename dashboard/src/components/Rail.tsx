import { NavLink } from "react-router-dom";
import type { ConnectionStatus } from "../lib/useVaultStream.js";

const CONNECTION_LABEL: Record<ConnectionStatus, string> = {
  idle: "Not connected",
  connecting: "Connecting",
  open: "Live",
  error: "Stream error",
  closed: "Disconnected",
};

function chipClass(c: ConnectionStatus): string {
  if (c === "open") return "chip live";
  if (c === "error") return "chip err";
  return "chip";
}

/** Persistent navigation rail. The numbered links encode the mandate lifecycle
 *  order (sign → execute → report); Overview sits outside the sequence. */
export function Rail({ connection }: { connection: ConnectionStatus }): JSX.Element {
  return (
    <aside className="rail">
      <NavLink to="/" className="wordmark" end>
        Cadence
        <span className="beat" aria-hidden="true" />
      </NavLink>

      <nav className="nav" aria-label="Primary">
        <span className="eyebrow nav-eyebrow">Desk</span>
        <NavLink to="/" end className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}>
          <span className="idx" aria-hidden="true">·</span>
          Overview
        </NavLink>
        <NavLink to="/portfolio" className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}>
          <span className="idx" aria-hidden="true">·</span>
          Portfolio
        </NavLink>

        <span className="eyebrow nav-eyebrow" style={{ marginTop: 14 }}>
          Lifecycle
        </span>
        <NavLink to="/mandate" className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}>
          <span className="idx" aria-hidden="true">01</span>
          Mandate
        </NavLink>
        <NavLink to="/execution" className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}>
          <span className="idx" aria-hidden="true">02</span>
          Execution
        </NavLink>
        <NavLink to="/report" className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}>
          <span className="idx" aria-hidden="true">03</span>
          Report
        </NavLink>
      </nav>

      <div className="rail-foot">
        <span className={chipClass(connection)}>
          <span className="dot" />
          {CONNECTION_LABEL[connection]}
        </span>
      </div>
    </aside>
  );
}
