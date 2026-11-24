import clsx from "clsx";
import { FolderGit2, FolderPlus, GitBranch, Network, RefreshCw, TerminalSquare, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import type { Workspace } from "../App";
import { FilePreview } from "../components/FilePreview";
import { FileTree } from "../components/FileTree";
import { Badge, Button, ConfirmButton, Empty, ErrorBanner, IconButton, SectionTitle, Tabs, ViewHeader } from "../components/ui";
import { api, type ActivityEntry, type Analysis, type GitStatus, type Issue, type Project } from "../lib/api";
import { importProject } from "../lib/importProject";
import { colorAt, errorText, formatBytes, formatNumber, primaryLanguage, timeAgo } from "../lib/util";

type Tab = "files" | "about" | "dependencies" | "findings";
const severityTone = { high: "red", medium: "amber", low: "blue", info: "neutral" } as const;

function About({ a }: { a: Analysis }) {
  const total = Math.max(1, a.languages.reduce((n, l) => n + l.lines, 0));
  return (
    <div className="mx-auto max-w-4xl space-y-6 px-6 py-5">
      <div>
        <p className="selectable text-sm leading-relaxed">{a.summary}</p>
        <div className="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-xs text-dim tabular-nums">
          <span>{formatNumber(a.file_count)} files</span>
          <span>{formatNumber(a.total_lines)} lines</span>
          <span>{formatBytes(a.total_bytes)}</span>
          {a.frameworks.length > 0 && <span>{a.frameworks.join(", ")}</span>}
          {a.truncated && <span className="text-warn">large project: only part of it was analysed</span>}
        </div>
      </div>
      <div className="grid grid-cols-[minmax(0,3fr)_minmax(0,2fr)] gap-10 max-[1000px]:grid-cols-1">
        <section aria-label="Languages">
          <SectionTitle>Languages</SectionTitle>
          <div className="mb-2 flex h-1.5 overflow-hidden rounded-full bg-raised">
            {a.languages.map((l, i) => <div key={l.name} title={l.name} style={{ width: `${(l.lines / total) * 100}%`, background: colorAt(i) }} />)}
          </div>
          <table className="w-full text-sm">
            <tbody>
              {a.languages.slice(0, 12).map((l, i) => (
                <tr key={l.name}>
                  <td className="py-0.5"><span className="mr-2 inline-block size-2 rounded-full" style={{ background: colorAt(i) }} />{l.name}</td>
                  <td className="py-0.5 text-right text-xs text-faint tabular-nums">{formatNumber(l.files)} files</td>
                  <td className="w-24 py-0.5 text-right text-xs text-dim tabular-nums">{formatNumber(l.lines)} lines</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section aria-label="Structure">
          <SectionTitle>Top-level folders</SectionTitle>
          <table className="w-full text-sm">
            <tbody>
              {a.top_dirs.map((d) => (
                <tr key={d.name}>
                  <td className="truncate py-0.5 font-mono text-xs">{d.name}</td>
                  <td className="py-0.5 text-right text-xs text-dim tabular-nums">{formatNumber(d.files)} files</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>
    </div>
  );
}

function Dependencies({ a }: { a: Analysis }) {
  if (!a.dependencies.length)
    return <Empty title="No dependency manifests found">NEXUS reads package.json, Cargo.toml, requirements.txt, pyproject.toml and go.mod.</Empty>;
  return (
    <div className="mx-auto max-w-5xl space-y-6 px-6 py-5">
      {a.dependencies.map((g) => (
        <section key={g.manifest}>
          <div className="flex items-center gap-2 border-b border-line pb-1.5">
            <span className="font-mono text-xs">{g.manifest}</span>
            <Badge>{g.ecosystem}</Badge>
            <span className="ml-auto text-xs text-faint">{g.deps.length} dependencies</span>
          </div>
          <div className="selectable grid grid-cols-2 gap-x-6 gap-y-1 pt-2 xl:grid-cols-3">
            {g.deps.map((d) => (
              <div key={d.name + d.dev} className="flex items-baseline gap-2 text-sm">
                <span className="truncate">{d.name}</span>
                {d.dev && <span className="text-2xs text-faint uppercase">dev</span>}
                <span className="ml-auto shrink-0 font-mono text-xs text-dim">{d.version}</span>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function Findings({ issues, onOpen }: { issues: Issue[]; onOpen: (path: string, line?: number) => void }) {
  if (!issues.length) return <Empty title="No findings">The analysis did not flag anything in this project.</Empty>;
  return (
    <ul className="divide-y divide-line">
      {issues.map((i, n) => (
        <li key={n} className="flex items-start gap-3 px-5 py-2.5">
          <div className="w-16 shrink-0 pt-0.5"><Badge tone={severityTone[i.severity]}>{i.severity}</Badge></div>
          <div className="min-w-0 flex-1">
            <div className="selectable text-sm break-words">{i.message}</div>
            {i.file && (
              <button className="mt-0.5 font-mono text-xs text-lav hover:underline" onClick={() => onOpen(i.file!, i.line ?? undefined)}>
                {i.file}{i.line ? `:${i.line}` : ""}
              </button>
            )}
          </div>
          <span className="shrink-0 text-xs text-faint">{i.kind}</span>
        </li>
      ))}
    </ul>
  );
}

function Detail({ project, ws }: { project: Project; ws: Workspace }) {
  const [tab, setTab] = useState<Tab>("files");
  const [file, setFile] = useState<{ path: string; line?: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const a = project.analysis;

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await fn();
      await ws.reloadProjects();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const openFile = (path: string, line?: number) => {
    setFile({ path, line });
    setTab("files");
  };

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <ViewHeader
        title={project.name}
        context={<span className="selectable truncate font-mono" title={project.path}>{project.path}</span>}
        actions={
          <>
            <IconButton label="Open a terminal in this project" disabled={!project.exists} onClick={() => ws.go("terminal")}><TerminalSquare size={14} /></IconButton>
            <IconButton label="Open Git for this project" disabled={!project.exists} onClick={() => ws.go("git")}><GitBranch size={14} /></IconButton>
            <IconButton label="Explore the code (Intelligence)" disabled={!project.exists} onClick={() => ws.go("intel")}><Network size={14} /></IconButton>
            <span className="mx-1 h-4 w-px bg-line" />
            <Button size="sm" onClick={() => run(() => api.analyzeProject(project.id))} busy={busy} disabled={!project.exists} title={`Analysed ${timeAgo(project.analyzed_at)}`}>
              <RefreshCw size={12} /> Re-analyse
            </Button>
            <ConfirmButton
              label=""
              icon={<Trash2 size={13} />}
              title="Remove from NEXUS (files are not deleted)"
              confirm="Remove from NEXUS — files stay on disk"
              onConfirm={() => run(() => api.removeProject(project.id))}
            />
          </>
        }
      />
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "files", label: "Files" },
          { id: "about", label: "About" },
          { id: "dependencies", label: "Dependencies", count: a?.dependencies.reduce((n, g) => n + g.deps.length, 0) },
          { id: "findings", label: "Findings", count: a?.issues.length },
        ]}
      />
      <div className="empty:hidden p-4 pb-0"><ErrorBanner message={error} /></div>
      {!project.exists ? (
        <Empty title="Folder not found">This project's folder was moved or deleted. Remove it and import it again from its new location.</Empty>
      ) : tab === "files" ? (
        <div className="flex min-h-0 flex-1">
          <div className="w-72 shrink-0 overflow-auto border-r border-line max-[1180px]:w-56">
            <FileTree key={project.id} projectId={project.id} selected={file?.path ?? null} onOpen={(p) => setFile({ path: p })} />
          </div>
          <div className="min-w-0 flex-1">
            {file ? <FilePreview projectId={project.id} path={file.path} line={file.line} /> : (
              <Empty title="Select a file to preview">Files open read-only. Use Search ({"⌘"}3) to find text across the project, or Intelligence to find symbols.</Empty>
            )}
          </div>
        </div>
      ) : !a ? (
        <Empty title="Not analysed yet">Run an analysis to see languages, dependencies and findings.</Empty>
      ) : (
        <div className="flex-1 overflow-auto">
          {tab === "about" && <About a={a} />}
          {tab === "dependencies" && <Dependencies a={a} />}
          {tab === "findings" && <Findings issues={a.issues} onOpen={openFile} />}
        </div>
      )}
    </div>
  );
}

/** Branch and change count for every project, read in the background. */
function useRepoStates(projects: Project[]) {
  const [states, setStates] = useState<Record<string, GitStatus | "error">>({});
  const key = projects.map((p) => p.id + p.exists).join();
  useEffect(() => {
    let live = true;
    for (const p of projects.filter((x) => x.exists)) {
      api.gitStatus(p.id).then((g) => live && setStates((s) => ({ ...s, [p.id]: g }))).catch(() => live && setStates((s) => ({ ...s, [p.id]: "error" })));
    }
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return states;
}

function repoLine(p: Project, g: GitStatus | "error" | undefined): { text: string; cls: string } {
  if (!p.exists) return { text: "folder missing", cls: "text-danger" };
  if (g === undefined) return { text: "…", cls: "text-faint" };
  if (g === "error") return { text: "Git status unavailable", cls: "text-faint" };
  if (!g.is_repo) return { text: "not a Git repository", cls: "text-faint" };
  const n = g.files.length;
  const conflicts = g.files.some((f) => f.conflicted);
  return {
    text: `${g.branch ?? "detached"} · ${conflicts ? "conflicts" : n === 0 ? "clean" : `${n} change${n === 1 ? "" : "s"}`}`,
    cls: conflicts ? "text-danger" : n ? "text-warn" : "text-dim",
  };
}

export default function ProjectsView({ ws }: { ws: Workspace }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const repos = useRepoStates(ws.projects);
  const [lastSeen, setLastSeen] = useState<Record<string, ActivityEntry>>({});
  useEffect(() => {
    api.listActivity(500).then((list) => {
      const latest: Record<string, ActivityEntry> = {};
      for (const e of list) if (e.project_id && !latest[e.project_id]) latest[e.project_id] = e;
      setLastSeen(latest);
    }).catch(() => undefined);
  }, [ws.projects.length]);

  const onImport = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const p = await importProject();
      if (p) {
        await ws.reloadProjects();
        ws.selectProject(p.id);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-72 shrink-0 flex-col border-r border-line max-[1180px]:w-60">
        <ViewHeader
          title="Projects"
          context={<span>{ws.projects.length}</span>}
          actions={<Button size="sm" onClick={onImport} busy={busy}><FolderPlus size={13} /> Import…</Button>}
        />
        <div className="empty:hidden p-2"><ErrorBanner message={error} /></div>
        <div className="flex-1 overflow-auto p-2">
          {ws.projects.map((p) => (
            <button
              key={p.id}
              data-project={p.name}
              aria-current={p.id === ws.projectId ? "true" : undefined}
              onClick={() => ws.selectProject(p.id)}
              className={clsx("mb-0.5 block w-full rounded-md px-2.5 py-1.5 text-left", p.id === ws.projectId ? "bg-raised" : "hover:bg-raised/60")}
            >
              <div className="flex items-baseline gap-2">
                <span className="truncate text-sm font-medium">{p.name}</span>
                <span className="ml-auto shrink-0 text-2xs text-faint">{p.analysis ? primaryLanguage(p.analysis.languages) : ""}</span>
              </div>
              <div className={clsx("truncate font-mono text-2xs", repoLine(p, repos[p.id]).cls)}>{repoLine(p, repos[p.id]).text}</div>
              <div className="truncate text-2xs text-faint">{lastSeen[p.id] ? `${lastSeen[p.id].message} · ${timeAgo(lastSeen[p.id].at)}` : `added ${timeAgo(p.added_at)}`}</div>
            </button>
          ))}
        </div>
      </aside>
      {ws.project ? (
        <Detail key={ws.project.id} project={ws.project} ws={ws} />
      ) : (
        <div className="flex-1">
          <Empty icon={<FolderGit2 size={30} />} title="Import your first project">
            Choose a local repository. NEXUS analyses its languages, frameworks, dependencies and potential problems entirely on this machine — no AI key needed.
          </Empty>
        </div>
      )}
    </div>
  );
}
