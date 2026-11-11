import clsx from "clsx";
import { Activity, Bot, FolderGit2, GitBranch, LayoutDashboard, ListChecks, Search } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Spinner } from "./components/ui";
import { api, type Project } from "./lib/api";
import { errorText } from "./lib/util";
import ActivityView from "./views/Activity";
import AIView from "./views/AI";
import Dashboard from "./views/Dashboard";
import GitView from "./views/GitView";
import SearchView from "./views/Search";
import TasksView from "./views/Tasks";

// Projects pulls in the Monaco editor, so it loads on demand.
const ProjectsView = lazy(() => import("./views/Projects"));

export type ViewId = "dashboard" | "projects" | "search" | "git" | "tasks" | "ai" | "activity";

const NAV: { id: ViewId; label: string; icon: typeof Bot }[] = [
  { id: "dashboard", label: "Overview", icon: LayoutDashboard },
  { id: "projects", label: "Projects", icon: FolderGit2 },
  { id: "search", label: "Search", icon: Search },
  { id: "git", label: "Git", icon: GitBranch },
  { id: "tasks", label: "Tasks", icon: ListChecks },
  { id: "ai", label: "AI Workspace", icon: Bot },
  { id: "activity", label: "Activity", icon: Activity },
];

/** Shared app state handed to every view. */
export interface Workspace {
  projects: Project[];
  projectId: string | null;
  project: Project | null;
  selectProject: (id: string | null) => void;
  reloadProjects: () => Promise<void>;
  go: (view: ViewId) => void;
}

export default function App() {
  const [view, setView] = useState<ViewId>("dashboard");
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(() => localStorage.getItem("nexus.project"));
  const [fatal, setFatal] = useState<string>();

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
    if (id) localStorage.setItem("nexus.project", id);
    else localStorage.removeItem("nexus.project");
  }, []);

  const project = projects.find((p) => p.id === projectId) ?? projects[0] ?? null;
  const ws: Workspace = { projects, projectId: project?.id ?? null, project, selectProject, reloadProjects, go: setView };

  return (
    <div className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col border-r border-line bg-panel">
        <div className="flex h-14 items-center gap-2.5 px-4">
          <div className="grid size-6 place-items-center rounded-md bg-accent text-[13px] font-black text-black">N</div>
          <span className="text-[15px] font-semibold tracking-wide">NEXUS</span>
        </div>
        <div className="flex-1 space-y-0.5 px-2">
          {NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => setView(id)}
              className={clsx(
                "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] font-medium",
                view === id ? "bg-raised text-ink" : "text-dim hover:bg-raised/60 hover:text-ink",
              )}
            >
              <Icon size={15} className={view === id ? "text-accent" : undefined} />
              {label}
            </button>
          ))}
        </div>
        <div className="border-t border-line p-3 text-[11px] leading-relaxed text-faint">
          Local-first. Your code and data stay on this machine.
        </div>
      </nav>
      <main className="flex min-w-0 flex-1 flex-col">
        {fatal ? (
          <div className="selectable space-y-3 p-8 text-red-300">
            <div>NEXUS could not reach its native core: {fatal}</div>
            <button className="rounded-md border border-line px-3 py-1.5 text-ink hover:bg-raised" onClick={() => void reloadProjects()}>
              Retry
            </button>
          </div>
        ) : (
          <Suspense fallback={<Spinner />}>
            {view === "dashboard" && <Dashboard ws={ws} />}
            {view === "projects" && <ProjectsView ws={ws} />}
            {view === "search" && <SearchView ws={ws} />}
            {view === "git" && <GitView ws={ws} />}
            {view === "tasks" && <TasksView ws={ws} />}
            {view === "ai" && <AIView ws={ws} />}
            {view === "activity" && <ActivityView />}
          </Suspense>
        )}
      </main>
    </div>
  );
}
