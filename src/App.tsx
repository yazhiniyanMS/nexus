import clsx from "clsx";
import { Activity, Bot, Command as CommandIcon, LifeBuoy, FlaskConical, FolderGit2, GitBranch, LayoutDashboard, ListChecks, Network, Search, Settings as SettingsIcon, TerminalSquare, Workflow } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { CommandPalette, type Command } from "./components/CommandPalette";
import { Welcome } from "./components/Welcome";
import { PeekDrawer } from "./components/PeekDrawer";
import { ProjectPicker } from "./components/ProjectPicker";
import { Spinner } from "./components/ui";
import { api, type Project } from "./lib/api";
import { importProject } from "./lib/importProject";
import { fromTerminal, isMac, modPressed, shortcut } from "./lib/platform";
import { errorText } from "./lib/util";
import ActivityView from "./views/Activity";
import AgentsView from "./views/Agents";
import Dashboard from "./views/Dashboard";
import GitView from "./views/GitView";
import SearchView from "./views/Search";
import TasksView from "./views/Tasks";

// These screens pull in the Monaco editor, so they load on demand.
const ProjectsView = lazy(() => import("./views/Projects"));
const IntelligenceView = lazy(() => import("./views/Intelligence"));
// The terminal emulator is only needed on its own screen.
const TerminalView = lazy(() => import("./views/Terminal"));
const ModelLabView = lazy(() => import("./views/ModelLab"));
const SettingsView = lazy(() => import("./views/Settings"));

export type ViewId = "dashboard" | "projects" | "intel" | "agents" | "terminal" | "models" | "search" | "git" | "tasks" | "activity" | "settings";
export interface PeekTarget { projectId: string; path: string; line?: number }

// Order defines the Ctrl/Cmd+1…9 shortcuts; screens after the ninth have none.
// Grouped by job. `ai` marks intelligence and model screens, which use the lavender accent.
const NAV: { id: ViewId; label: string; icon: typeof Bot; group: string; ai?: boolean }[] = [
  { id: "dashboard", label: "Overview", icon: LayoutDashboard, group: "Workspace" },
  { id: "projects", label: "Projects", icon: FolderGit2, group: "Workspace" },
  { id: "search", label: "Search", icon: Search, group: "Workspace" },
  { id: "git", label: "Git", icon: GitBranch, group: "Workspace" },
  { id: "terminal", label: "Terminal", icon: TerminalSquare, group: "Workspace" },
  { id: "tasks", label: "Tasks", icon: ListChecks, group: "Automation" },
  { id: "agents", label: "Agents", icon: Workflow, group: "Automation", ai: true },
  { id: "intel", label: "Intelligence", icon: Network, group: "Models", ai: true },
  { id: "models", label: "Model Lab", icon: FlaskConical, group: "Models", ai: true },
  { id: "activity", label: "Activity", icon: Activity, group: "" },
];

/** Shared app state handed to every view. */
export interface Workspace {
  projects: Project[];
  projectId: string | null;
  project: Project | null;
  selectProject: (id: string | null) => void;
  reloadProjects: () => Promise<void>;
  go: (view: ViewId) => void;
  /** Open (or close, with null) the slide-over file viewer from anywhere. */
  peek: (target: PeekTarget | null) => void;
  peekTarget: PeekTarget | null;
  /** Send a file to the Intelligence "Ask" tab as the focus of a question. */
  askAbout: (path: string | null) => void;
  askFocus: string | null;
}


export default function App() {
  const [view, setViewState] = useState<ViewId>("dashboard");
  const setView = useCallback((v: ViewId) => {
    setViewState(v);
    void api.appPrefsSetLastView(v).catch(() => undefined);
  }, []);
  useEffect(() => {
    api.appPrefs().then((p) => {
      if (p.start_view === "last" && p.last_view) setViewState(p.last_view as ViewId);
    }).catch(() => undefined);
  }, []);
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(() => localStorage.getItem("nexus.project"));
  const [fatal, setFatal] = useState<string>();
  const [palette, setPalette] = useState(false);
  const [peekTarget, setPeek] = useState<PeekTarget | null>(null);
  const [askFocus, setAskFocus] = useState<string | null>(null);
  const [about, setAbout] = useState(false);
  // null until the core says whether the welcome screen was already seen.
  const [welcome, setWelcome] = useState<boolean | null>(null);

  useEffect(() => {
    api.onboardingDone().then((done) => setWelcome(!done)).catch(() => setWelcome(false));
  }, []);

  const reloadProjects = useCallback(async () => {
    try {
      setProjects(await api.listProjects());
      setFatal(undefined);
    } catch (e) {
      setFatal(errorText(e));
    }
  }, []);

  useEffect(() => {
    void reloadProjects();
  }, [reloadProjects]);

  const selectProject = useCallback((id: string | null) => {
    setProjectId(id);
    setPeek(null);
    setAskFocus(null);
    if (id) localStorage.setItem("nexus.project", id);
    else localStorage.removeItem("nexus.project");
  }, []);

  const askAbout = useCallback((path: string | null) => {
    setAskFocus(path);
    if (path) setView("intel");
  }, []);

  const project = projects.find((p) => p.id === projectId) ?? projects[0] ?? null;
  const ws: Workspace = {
    projects, projectId: project?.id ?? null, project, selectProject, reloadProjects, go: setView,
    peek: setPeek, peekTarget, askAbout, askFocus,
  };

  // Global shortcuts: Ctrl/Cmd+K palette, Ctrl/Cmd+1…9 screens, Escape closes overlays.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Ctrl+Shift+P opens the palette everywhere. Where Control is the app modifier (Windows,
      // Linux), Control combinations typed in a terminal belong to the shell, not to NEXUS.
      const paletteAlt = e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "p";
      const mod = modPressed(e) && !(!isMac && fromTerminal(e));
      if (paletteAlt || (mod && e.key.toLowerCase() === "k")) {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (mod && e.key === ",") {
        e.preventDefault();
        setView("settings");
      } else if (mod && /^[1-9]$/.test(e.key) && NAV[Number(e.key) - 1]) {
        e.preventDefault();
        setView(NAV[Number(e.key) - 1].id);
      } else if (e.key === "Escape") {
        setPalette(false);
        setPeek(null);
        setAbout(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const actions: Command[] = useMemo(() => {
    const list: Command[] = [
      {
        id: "action:import", label: "Import project…", group: "Actions",
        run: () => void importProject().then(async (p) => {
          if (p) {
            await reloadProjects();
            selectProject(p.id);
            setView("projects");
          }
        }).catch(() => undefined),
      },
    ];
    if (project) {
      list.push(
        { id: "action:index", label: `Rebuild code index for ${project.name}`, group: "Actions", run: () => void api.rebuildIndex(project.id).then(() => setView("intel")).catch(() => undefined) },
        { id: "action:analyse", label: `Re-analyse ${project.name}`, group: "Actions", run: () => void api.analyzeProject(project.id).then(reloadProjects).catch(() => undefined) },
        { id: "action:map", label: "Open architecture map", group: "Actions", run: () => setView("intel") },
        { id: "action:terminal", label: `Open terminal in ${project.name}`, group: "Actions", run: () => setView("terminal") },
      );
    }
    list.push({ id: "action:models", label: "Open Model Lab", group: "Actions", run: () => setView("models") });
    list.push({ id: "action:about", label: "About NEXUS, diagnostics and feedback", group: "Actions", run: () => setAbout(true) });
    return list;
  }, [project, reloadProjects, selectProject]);

  return (
    <div className="relative flex h-full">
      <nav className="flex w-52 shrink-0 flex-col border-r border-line bg-panel">
        <div className="flex h-11 shrink-0 items-center gap-2.5 px-4">
          <div className="grid size-6 place-items-center rounded-md bg-accent text-sm font-black text-black">N</div>
          <span className="text-md font-semibold tracking-wide">NEXUS</span>
        </div>
        <div className="space-y-2 px-2 pb-3">
          {projects.length > 0 && <ProjectPicker ws={ws} className="w-full" />}
          <button
            onClick={() => setPalette(true)}
            className="flex h-8 w-full items-center gap-2 rounded-md border border-line bg-bg px-2.5 text-xs text-faint hover:text-dim"
          >
            <CommandIcon size={12} /> Search or run…
            <kbd className="ml-auto font-sans text-2xs">{shortcut("K")}</kbd>
          </button>
        </div>
        <div className="flex-1 space-y-0.5 overflow-auto px-2">
          {NAV.map(({ id, label, icon: Icon, ai, group }, i) => (
            <div key={id}>
            {group && (i === 0 || NAV[i - 1].group !== group) && <div className="px-2.5 pt-3 pb-1 text-2xs font-medium text-faint">{group}</div>}
            {!group && <div className="my-2 border-t border-line" />}
            <button
              onClick={() => setView(id)}
              aria-current={view === id ? "page" : undefined}
              title={i < 9 ? `${label} (${shortcut(String(i + 1))})` : label}
              className={clsx(
                "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium",
                view === id ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink",
              )}
            >
              <Icon size={15} className={view === id ? (ai ? "text-lav" : "text-accent") : undefined} />
              {label}
            </button>
            </div>
          ))}
        </div>
        <div className="border-t border-line p-2">
          <button
            onClick={() => setView("settings")}
            aria-current={view === "settings" ? "page" : undefined}
            title={`Settings (${shortcut(",")})`}
            className={clsx("flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium", view === "settings" ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink")}
          >
            <SettingsIcon size={15} /> Settings
          </button>
          <button
            onClick={() => setAbout(true)}
            className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-sm font-medium text-dim hover:bg-raised/60 hover:text-ink"
          >
            <LifeBuoy size={15} /> About &amp; feedback
          </button>
        </div>
      </nav>
      <main className="relative flex min-w-0 flex-1 flex-col">
        {fatal ? (
          <div className="selectable space-y-3 p-8 text-danger">
            <div>NEXUS could not reach its native core: {fatal}</div>
            <button className="rounded-md border border-line px-3 py-1.5 text-ink hover:bg-raised" onClick={() => void reloadProjects()}>
              Retry
            </button>
          </div>
        ) : (
          <Suspense fallback={<Spinner />}>
            {view === "dashboard" && <Dashboard ws={ws} />}
            {view === "projects" && <ProjectsView ws={ws} />}
            {view === "intel" && <IntelligenceView ws={ws} />}
            {view === "agents" && <AgentsView ws={ws} />}
            {view === "terminal" && <TerminalView ws={ws} />}
            {view === "models" && <ModelLabView ws={ws} />}
            {view === "search" && <SearchView ws={ws} />}
            {view === "git" && <GitView ws={ws} />}
            {view === "tasks" && <TasksView ws={ws} />}
            {view === "settings" && <SettingsView ws={ws} views={NAV} onAbout={() => setAbout(true)} />}
            {view === "activity" && <ActivityView />}
          </Suspense>
        )}
        <PeekDrawer ws={ws} />
      </main>
      {welcome && !fatal && <Welcome ws={ws} onDone={() => setWelcome(false)} />}
      {about && <AboutDialog onClose={() => setAbout(false)} />}
      {palette && <CommandPalette ws={ws} views={NAV} actions={actions} onClose={() => setPalette(false)} />}
    </div>
  );
}
