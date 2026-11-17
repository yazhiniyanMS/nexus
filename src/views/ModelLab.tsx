// Model Lab: discover, run, measure and compare models, local first.
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Workspace } from "../App";
import { ErrorBanner, PageHeader, Tabs } from "../components/ui";
import { api, type Library } from "../lib/api";
import { errorText, useAsync } from "../lib/util";
import BenchmarksTab from "./lab/Benchmarks";
import CompareTab from "./lab/Compare";
import LibraryTab from "./lab/Library";
import PlaygroundTab from "./lab/Playground";
import ProvidersTab from "./lab/Providers";
import RuntimesTab from "./lab/Runtimes";
import type { LabCtx, LabTab, LabTarget } from "./lab/shared";

const TABS: { id: LabTab; label: string }[] = [
  { id: "library", label: "Library" },
  { id: "runtimes", label: "Runtimes" },
  { id: "playground", label: "Playground" },
  { id: "benchmarks", label: "Benchmarks" },
  { id: "compare", label: "Compare" },
  { id: "providers", label: "Providers" },
];
const TAB_KEY = "nexus.lab.tab";

const savedTab = (): LabTab => {
  const t = localStorage.getItem(TAB_KEY);
  return TABS.some((x) => x.id === t) ? (t as LabTab) : "library";
};

export default function ModelLabView({ ws }: { ws: Workspace }) {
  const [tab, setTab] = useState<LabTab>(savedTab);
  // Tabs are created on first visit and then kept, so a running generation or benchmark view
  // is not lost by looking at another tab.
  const [visited, setVisited] = useState<Set<LabTab>>(() => new Set([savedTab()]));
  const [targets, setTargets] = useState<Partial<Record<LabTab, LabTarget>>>({});
  const providers = useAsync(api.listProviders, []);
  const kinds = useAsync(api.labKinds, []);
  const settings = useAsync(api.labSettings, []);
  const [library, setLibrary] = useState<Library>();
  const [libraryBusy, setLibraryBusy] = useState(false);
  const [libraryError, setLibraryError] = useState<string>();

  const refreshLibrary = useCallback(async () => {
    setLibraryBusy(true);
    try {
      setLibrary(await api.labModelsRefresh());
      setLibraryError(undefined);
    } catch (e) {
      setLibraryError(errorText(e));
    } finally {
      setLibraryBusy(false);
    }
  }, []);

  // Show what is cached at once, then ask the runtimes what they have now.
  useEffect(() => {
    api.labModels().then((cached) => setLibrary((current) => current ?? cached)).catch(() => undefined);
    void refreshLibrary();
  }, [refreshLibrary]);

  const go = useCallback((next: LabTab, target?: LabTarget) => {
    if (target) setTargets((t) => ({ ...t, [next]: { ...target } }));
    setVisited((v) => new Set(v).add(next));
    setTab(next);
    localStorage.setItem(TAB_KEY, next);
  }, []);

  const ctx: LabCtx = useMemo(() => ({
    ws,
    providers: providers.data ?? [],
    kinds: kinds.data ?? [],
    settings: settings.data,
    library,
    libraryBusy,
    libraryError,
    reloadProviders: providers.reload,
    refreshLibrary,
    reloadSettings: settings.reload,
    go,
  }), [ws, providers.data, providers.reload, kinds.data, settings.data, settings.reload, library, libraryBusy, libraryError, refreshLibrary, go]);

  const list = providers.data ?? [];
  const local = list.filter((p) => p.enabled && p.local).length;
  const remote = list.filter((p) => p.enabled && !p.local).length;
  const modelCount = library?.models.filter((m) => m.status !== "missing").length;
  const show = (id: LabTab) => (tab === id ? "flex min-h-0 flex-1 flex-col" : "hidden");

  return (
    <>
      <PageHeader
        title="Model Lab"
        subtitle={
          <span data-testid="lab-summary">
            {modelCount === undefined ? "Loading models…" : `${modelCount} model${modelCount === 1 ? "" : "s"}`} · {local} local connection{local === 1 ? "" : "s"}
            {remote > 0 ? ` · ${remote} remote (asks before sending)` : " · nothing leaves this machine"}
          </span>
        }
      />
      <Tabs tabs={TABS} value={tab} onChange={(t) => go(t)} />
      <div className="empty:hidden px-4 pt-3"><ErrorBanner message={providers.error ?? kinds.error ?? settings.error} /></div>
      {visited.has("library") && <div className={show("library")}><LibraryTab ctx={ctx} /></div>}
      {visited.has("runtimes") && <div className={show("runtimes")}><RuntimesTab ctx={ctx} /></div>}
      {visited.has("playground") && <div className={show("playground")}><PlaygroundTab ctx={ctx} target={targets.playground} /></div>}
      {visited.has("benchmarks") && <div className={show("benchmarks")}><BenchmarksTab ctx={ctx} target={targets.benchmarks} /></div>}
      {visited.has("compare") && <div className={show("compare")}><CompareTab ctx={ctx} target={targets.compare} /></div>}
      {visited.has("providers") && <div className={show("providers")}><ProvidersTab ctx={ctx} /></div>}
    </>
  );
}
