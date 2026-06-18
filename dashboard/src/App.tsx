import { useEffect, useMemo, useState } from "react";
import { HashRouter, Routes, Route, Outlet, useOutletContext } from "react-router-dom";
import { loadDashboardConfig, type DashboardConfig } from "./config.js";
import { useVaultStream, type StreamResult } from "./lib/useVaultStream.js";
import { Rail } from "./components/Rail.js";
import { Overview } from "./screens/Overview.js";
import { Portfolio } from "./screens/Portfolio.js";
import { CreateMandate } from "./screens/CreateMandate.js";
import { LiveExecution } from "./screens/LiveExecution.js";
import { FinalReport } from "./screens/FinalReport.js";

export interface DeskContext {
  config: DashboardConfig;
  stream: StreamResult;
  nowMs: number;
}

export function useDesk(): DeskContext {
  return useOutletContext<DeskContext>();
}

function Layout({ config }: { config: DashboardConfig }): JSX.Element {
  const stream = useVaultStream(config);
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  const ctx: DeskContext = { config, stream, nowMs };
  return (
    <div className="shell">
      <Rail connection={stream.connection} />
      <main className="content">
        <Outlet context={ctx} />
      </main>
    </div>
  );
}

export function App(): JSX.Element {
  const config = useMemo(() => loadDashboardConfig(), []);
  return (
    <HashRouter>
      <Routes>
        <Route element={<Layout config={config} />}>
          <Route index element={<Overview />} />
          <Route path="portfolio" element={<Portfolio />} />
          <Route path="mandate" element={<CreateMandate config={config} />} />
          <Route path="execution" element={<LiveExecution />} />
          <Route path="report" element={<FinalReport />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
