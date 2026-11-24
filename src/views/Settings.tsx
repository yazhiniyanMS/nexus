// Settings: everything that configures NEXUS itself, in one place.
import clsx from "clsx";
import { useEffect, useMemo, useState } from "react";
import type { ViewId, Workspace } from "../App";
import { ProviderManager } from "../components/ProviderManager";
import { Button, ErrorBanner, Kbd, Select, Spinner, ViewHeader } from "../components/ui";
import { api, type AppPrefs, type PlatformInfo } from "../lib/api";
import { isMac, shortcut } from "../lib/platform";
import { errorText, formatNumber, useAsync } from "../lib/util";
import { Defaults } from "./lab/Defaults";
import { Execution } from "./settings/Execution";
import type { LabCtx, LabTab } from "./lab/shared";
import { useLabData } from "./lab/useLabData";
import { LAB_TAB_KEY } from "./ModelLab";

export type SettingsSection = "connections" | "defaults" | "execution" | "usage" | "keyboard" | "general";

const SECTIONS: { id: SettingsSection; label: string; hint: string }[] = [
  { id: "connections", label: "Model connections", hint: "Local runtimes and API providers" },
  { id: "defaults", label: "Model defaults", hint: "Default model and generation settings" },
  { id: "execution", label: "Execution", hint: "Sandboxing, time limits, environment" },
  { id: "usage", label: "Token usage", hint: "Requests and tokens per model" },
  { id: "keyboard", label: "Keyboard", hint: "Shortcuts available everywhere" },
  { id: "general", label: "General", hint: "Start screen, data folder, welcome" },
];

function Usage() {
  const usage = useAsync(api.usageSummary, []);
  if (usage.error) return <ErrorBanner message={usage.error} />;
  if (!usage.data) return <Spinner />;
  if (usage.data.length === 0) {
    return <p className="text-sm text-dim">Nothing recorded yet. Every request to a model (Playground, project questions, agents, task planning) adds the tokens its runtime reports.</p>;
  }
  return (
    <table data-testid="usage-table" className="selectable w-full text-sm [&_td]:pr-3 [&_th]:pr-3">
      <thead className="text-left text-xs text-faint">
        <tr className="border-b border-line"><th className="py-2 font-medium">Connection</th><th className="font-medium">Model</th><th className="text-right font-medium">Requests</th><th className="text-right font-medium">Input tokens</th><th className="text-right font-medium">Output tokens</th></tr>
      </thead>
      <tbody className="divide-y divide-line">
        {usage.data.map((u) => (
          <tr key={u.provider + u.model}>
            <td className="py-2">{u.provider}</td>
            <td className="font-mono text-xs">{u.model}</td>
            <td className="text-right tabular-nums">{formatNumber(u.requests)}</td>
            <td className="text-right tabular-nums">{formatNumber(u.input_tokens)}</td>
            <td className="text-right tabular-nums">{formatNumber(u.output_tokens)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Every shortcut here is handled by the app; this list is not aspirational. */
function Keyboard({ views }: { views: { id: ViewId; label: string }[] }) {
  const mod = isMac ? "⌘" : "Ctrl";
  const rows: [string, string][] = [
    [shortcut("K"), "Command palette: screens, projects, actions and code symbols"],
    ...(isMac ? [] : ([["Ctrl+Shift+P", "Command palette, also from inside a terminal"]] as [string, string][])),
    [shortcut(","), "Settings"],
    ...views.slice(0, 9).map((v, i) => [shortcut(String(i + 1)), v.label] as [string, string]),
    ["Esc", "Close the palette, the file drawer or a dialog; cancel a pending confirmation"],
    ["↑  ↓  Enter", "Move through and open items in the palette"],
    [`${mod}+Enter`, "Commit (Git), when the commit message is focused"],
    ["Enter", "Run the prompt (Playground) or send a question; Shift+Enter adds a line"],
  ];
  return (
    <table className="w-full text-sm">
      <tbody className="divide-y divide-line">
        {rows.map(([keys, what]) => (
          <tr key={keys + what}>
            <td className="w-40 py-2"><Kbd>{keys}</Kbd></td>
            <td className="py-2 text-dim">{what}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function General({ onAbout }: { onAbout: () => void }) {
  const prefs = useAsync(api.appPrefs, []);
  const [info, setInfo] = useState<PlatformInfo>();
  const [note, setNote] = useState<string>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    api.platformInfo().then(setInfo).catch((e) => setError(errorText(e)));
  }, []);

  const act = async (fn: () => Promise<unknown>, done: string) => {
    setError(undefined);
    setNote(undefined);
    try {
      await fn();
      setNote(done);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="space-y-6">
      <ErrorBanner message={error ?? prefs.error} />
      <div className="grid grid-cols-[12rem_1fr] items-start gap-x-6 gap-y-5 text-sm">
        <label htmlFor="start-view" className="pt-1.5 text-dim">Open NEXUS on</label>
        <div>
          <Select
            id="start-view"
            className="w-64!"
            value={prefs.data?.start_view ?? "dashboard"}
            disabled={!prefs.data}
            onChange={(e) => void act(async () => prefs.setData(await api.appPrefsSetStart(e.target.value as AppPrefs["start_view"])), "Saved.")}
          >
            <option value="dashboard">Overview</option>
            <option value="last">The screen I used last</option>
          </Select>
        </div>

        <div className="text-dim">Data folder</div>
        <div className="min-w-0">
          <div data-testid="data-dir" className="selectable font-mono text-xs break-all">{info?.data_dir ?? "…"}</div>
          <div className="mt-1 text-xs text-faint">
            Database, terminal output and downloaded models. {info?.data_dir_overridden ? "Set by NEXUS_DATA_DIR." : "Set NEXUS_DATA_DIR before starting NEXUS to use another folder."}
          </div>
        </div>

        <div className="text-dim">API keys</div>
        <div className="text-dim">{info?.secure_storage ?? "…"}</div>

        <div className="pt-1.5 text-dim">Welcome screen</div>
        <div>
          <Button onClick={() => void act(api.onboardingReset, "The welcome screen will show the next time NEXUS starts.")}>Show it again on next start</Button>
        </div>

        <div className="pt-1.5 text-dim">About</div>
        <div>
          <Button onClick={onAbout}>Version, diagnostics and feedback…</Button>
        </div>
      </div>
      {note && <div role="status" className="text-xs text-ok">{note}</div>}
    </div>
  );
}

export default function SettingsView({ ws, views, initial, onAbout }: { ws: Workspace; views: { id: ViewId; label: string }[]; initial?: SettingsSection; onAbout: () => void }) {
  const [section, setSection] = useState<SettingsSection>(initial ?? "connections");
  useEffect(() => {
    if (initial) setSection(initial);
  }, [initial]);
  const data = useLabData();

  // The connection and defaults panels come from the Model Lab; links inside them open its tabs.
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
    go: (tab: LabTab) => {
      if (tab === "providers") return setSection("connections");
      localStorage.setItem(LAB_TAB_KEY, tab);
      ws.go("models");
    },
  }), [ws, data]);

  const reloadConnections = async () => {
    await data.reloadProviders();
    await data.reloadSettings();
    await data.refreshLibrary();
  };
  const current = SECTIONS.find((s) => s.id === section)!;

  return (
    <>
      <ViewHeader title="Settings" />
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Settings sections" className="w-56 shrink-0 space-y-0.5 border-r border-line p-2">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              data-section={s.id}
              aria-current={section === s.id ? "page" : undefined}
              onClick={() => setSection(s.id)}
              className={clsx("block w-full rounded-md px-2.5 py-1.5 text-left", section === s.id ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink")}
            >
              <div className="text-sm font-medium">{s.label}</div>
              <div className="truncate text-2xs text-faint">{s.hint}</div>
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-auto">
          <div className="w-full max-w-3xl space-y-4 px-8 py-5">
            <h2 className="text-md font-semibold">{current.label}</h2>
            <ErrorBanner message={data.error} />
            {section === "connections" && (
              <ProviderManager
                providers={data.providers}
                reload={reloadConnections}
                intro="Requests go straight from this machine to a connection's address. Local connections never leave it; remote ones are marked, and every screen asks before sending anything to them. API keys are kept in the system keychain and removed from error messages."
              />
            )}
            {section === "defaults" && <Defaults ctx={ctx} />}
            {section === "execution" && <Execution />}
            {section === "usage" && <Usage />}
            {section === "keyboard" && <Keyboard views={views} />}
            {section === "general" && <General onAbout={onAbout} />}
          </div>
        </div>
      </div>
    </>
  );
}
