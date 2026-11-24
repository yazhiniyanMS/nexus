// Model Lab: discover, run, measure and compare models, local first.
import { useCallback, useMemo, useState } from "react";
import type { Workspace } from "../App";
import { ErrorBanner, Tabs, ViewHeader } from "../components/ui";
import BenchmarksTab from "./lab/Benchmarks";
import CompareTab from "./lab/Compare";
import LibraryTab from "./lab/Library";
import PlaygroundTab from "./lab/Playground";
import RuntimesTab from "./lab/Runtimes";
import type { LabCtx, LabTab, LabTarget } from "./lab/shared";
import { useLabData } from "./lab/useLabData";

// Connections and defaults live in Settings; "providers" links there.
type LabScreen = Exclude<LabTab, "providers">;
const TABS: { id: LabScreen; label: string }[] = [
  { id: "library", label: "Library" },
  { id: "runtimes", label: "Runtimes" },
  { id: "playground", label: "Playground" },
  { id: "benchmarks", label: "Benchmarks" },
  { id: "compare", label: "Compare" },
];
export const LAB_TAB_KEY = "nexus.lab.tab";

const savedTab = (): LabScreen => {
  const t = localStorage.getItem(LAB_TAB_KEY);
  return TABS.some((x) => x.id === t) ? (t as LabScreen) : "library";
};

export default function ModelLabView({ ws }: { ws: Workspace }) {
  const [tab, setTab] = useState<LabScreen>(savedTab);
  // Tabs are created on first visit and then kept, so a running generation or benchmark view
  // is not lost by looking at another tab.
  const [visited, setVisited] = useState<Set<LabScreen>>(() => new Set([savedTab()]));
  const [targets, setTargets] = useState<Partial<Record<LabScreen, LabTarget>>>({});
  const data = useLabData();

  const go = useCallback((next: LabTab, target?: LabTarget) => {
    if (next === "providers") return ws.go("settings");
    if (target) setTargets((t) => ({ ...t, [next]: { ...target } }));
    setVisited((v) => new Set(v).add(next));
    setTab(next);
    localStorage.setItem(LAB_TAB_KEY, next);
  }, [ws]);

  const ctx: LabCtx = useMemo(() => ({
    ws,
    providers: data.providers,
    kinds: data.kinds,
    settings: data.settings,
    library: data.library,
    libraryBusy: data.libraryBusy,
    libraryError: data.libraryError,
    reloadProviders: data.reloadProviders,
    refreshLibrary: data.refreshLibrary,
    reloadSettings: data.reloadSettings,
    go,
  }), [ws, data, go]);

  const local = data.providers.filter((p) => p.enabled && p.local).length;
  const remote = data.providers.filter((p) => p.enabled && !p.local).length;
  const modelCount = data.library?.models.filter((m) => m.status !== "missing").length;
  const show = (id: LabScreen) => (tab === id ? "flex min-h-0 flex-1 flex-col" : "hidden");

  return (
    <>
      <ViewHeader
        title="Model Lab"
        context={
          <span data-testid="lab-summary">
            {modelCount === undefined ? "Loading models…" : `${modelCount} model${modelCount === 1 ? "" : "s"}`} · {local} local connection{local === 1 ? "" : "s"}
            {remote > 0 ? ` · ${remote} remote (asks before sending)` : " · nothing leaves this machine"}
          </span>
        }
        actions={<button className="text-xs text-dim hover:text-ink" onClick={() => ws.go("settings")}>Connections and defaults…</button>}
      />
      <Tabs tabs={TABS} value={tab} onChange={(t) => go(t)} />
      <div className="empty:hidden px-4 pt-3"><ErrorBanner message={data.error} /></div>
      {visited.has("library") && <div className={show("library")}><LibraryTab ctx={ctx} /></div>}
      {visited.has("runtimes") && <div className={show("runtimes")}><RuntimesTab ctx={ctx} /></div>}
      {visited.has("playground") && <div className={show("playground")}><PlaygroundTab ctx={ctx} target={targets.playground} /></div>}
      {visited.has("benchmarks") && <div className={show("benchmarks")}><BenchmarksTab ctx={ctx} target={targets.benchmarks} /></div>}
      {visited.has("compare") && <div className={show("compare")}><CompareTab ctx={ctx} target={targets.compare} /></div>}
    </>
  );
}
