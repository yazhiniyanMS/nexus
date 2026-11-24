// Overview: the state of the project you are working on, and what needs you.
import clsx from "clsx";
import { AlertTriangle, ArrowDown, ArrowUp, FileCode2, FileText, FolderPlus, Loader2, GitBranch, Network, RefreshCw, TerminalSquare, Workflow } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { Workspace } from "../App";
import { Button, Empty, ErrorBanner, SectionTitle, Spinner, Status, type StatusKind } from "../components/ui";
import { AGENT_CHANGED, api, type AgentRun, type FileStatus, type Issue } from "../lib/api";
import { importProject } from "../lib/importProject";
import { shortcut } from "../lib/platform";
import { errorText, formatNumber, primaryLanguage, timeAgo, useAsync } from "../lib/util";
import { listen } from "@tauri-apps/api/event";

export const runStatus: Record<AgentRun["status"], StatusKind> = {
  queued: "queued", running: "running", paused: "waiting", completed: "done", failed: "failed",
  cancelled: "cancelled", rejected: "cancelled", discarded: "cancelled", interrupted: "waiting",
};

/** The one-letter Git code for a changed file, coloured by what happened to it. */
export function changeCode(f: FileStatus): { code: string; cls: string; label: string } {
  if (f.conflicted) return { code: "!", cls: "text-danger", label: "conflict" };
  if (f.untracked) return { code: "U", cls: "text-ok", label: "untracked" };
  const c = f.index !== "." ? f.index : f.worktree;
  if (c === "A") return { code: "A", cls: "text-ok", label: "added" };
  if (c === "D") return { code: "D", cls: "text-danger", label: "deleted" };
  if (c === "R") return { code: "R", cls: "text-info", label: "renamed" };
  return { code: "M", cls: "text-warn", label: "modified" };
}

const severityOrder: Record<Issue["severity"], number> = { high: 0, medium: 1, low: 2, info: 3 };
const severityCls: Record<Issue["severity"], string> = { high: "text-danger", medium: "text-warn", low: "text-dim", info: "text-faint" };

function Action({ icon, label, keys, onClick }: { icon: ReactNode; label: string; keys?: string; onClick: () => void }) {
  return (
    <button onClick={onClick} className="flex h-8 items-center gap-2 rounded-md px-2.5 text-sm text-dim hover:bg-raised hover:text-ink">
      {icon}
      {label}
      {keys && <span className="text-2xs text-faint">{keys}</span>}
    </button>
  );
}

function NoProject({ ws }: { ws: Workspace }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  return (
    <Empty icon={<FolderPlus size={28} />} title="Import a project to start">
      <p className="mb-3">NEXUS works on folders on this machine: it reads their structure, Git history and code. Nothing is uploaded.</p>
      <ErrorBanner message={error} />
      <Button
        variant="primary"
        busy={busy}
        onClick={async () => {
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
        }}
      >
        <FolderPlus size={14} /> Import project…
      </Button>
    </Empty>
  );
}

export default function Dashboard({ ws }: { ws: Workspace }) {
  const project = ws.project;
  const id = project?.id ?? "";
  const git = useAsync(() => (id && project?.exists ? api.gitStatus(id) : Promise.resolve(null)), [id]);
  const index = useAsync(() => (id ? api.indexStatus(id) : Promise.resolve(null)), [id]);
  const runs = useAsync(api.listAgentRuns, []);
  const activity = useAsync(() => api.listActivity(300), [id]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const [reporting, setReporting] = useState(false);

  useEffect(() => {
    const sub = listen(AGENT_CHANGED, () => void runs.reload());
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const projectRuns = useMemo(() => (runs.data ?? []).filter((r) => r.project_id === id), [runs.data, id]);
  const attention = projectRuns.filter((r) => r.pending_approvals > 0 || ["running", "queued", "paused", "interrupted", "failed"].includes(r.status));
  const recent = useMemo(() => (activity.data ?? []).filter((a) => a.project_id === id).slice(0, 14), [activity.data, id]);
  const findings = useMemo(
    () => [...(project?.analysis?.issues ?? [])].sort((a, b) => severityOrder[a.severity] - severityOrder[b.severity]),
    [project?.analysis],
  );

  if (!project) return <NoProject ws={ws} />;

  const a = project.analysis;
  const g = git.data;
  const changes = g?.files ?? [];
  const conflicts = changes.filter((f) => f.conflicted);
  const staged = changes.filter((f) => !f.untracked && !f.conflicted && f.index !== ".");
  const serious = findings.filter((f) => f.severity === "high" || f.severity === "medium");

  /** The shareable report needs the code index; build it first if there is none. */
  const report = async () => {
    setReporting(true);
    setError(undefined);
    setNotice(undefined);
    try {
      if (!index.data?.indexed_at) {
        await api.rebuildIndex(project.id);
        await index.reload();
      }
      const saved = await api.saveProjectReport(project.id, true);
      if (saved) setNotice(`Report saved to ${saved} and opened in your browser. It lists paths and names, no source code.`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setReporting(false);
    }
  };

  const reanalyse = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await api.analyzeProject(project.id);
      await ws.reloadProjects();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="overview" className="flex-1 overflow-auto">
      <div className="mx-auto w-full max-w-6xl px-8 pt-7 pb-10">
        {/* Identity: the one large thing on the page */}
        <header className="flex items-start gap-6">
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-lg font-semibold">{project.name}</h1>
            <div className="selectable mt-0.5 truncate font-mono text-xs text-faint" title={project.path}>{project.path}</div>
            <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-dim">
              {!project.exists && <span className="flex items-center gap-1 text-danger"><AlertTriangle size={12} /> Folder not found on disk</span>}
              {a && <span>{primaryLanguage(a.languages) ?? "No code detected"}{a.frameworks.length ? ` · ${a.frameworks.slice(0, 4).join(", ")}` : ""}</span>}
              {a && <span className="tabular-nums">{formatNumber(a.file_count)} files · {formatNumber(a.total_lines)} lines</span>}
              <span>analysed {timeAgo(project.analyzed_at)}</span>
              {index.data && <span>{index.data.indexed_at ? `code index ${timeAgo(index.data.indexed_at)}` : "code not indexed yet"}</span>}
            </div>
          </div>
          <Button size="sm" busy={busy} disabled={!project.exists} onClick={() => void reanalyse()}><RefreshCw size={12} /> Re-analyse</Button>
        </header>

        <nav aria-label="Project actions" className="mt-4 -ml-2.5 flex flex-wrap items-center gap-1 border-b border-line pb-3">
          <Action icon={<FileCode2 size={14} />} label="Files" onClick={() => ws.go("projects")} />
          <Action icon={<GitBranch size={14} />} label="Git" keys={shortcut("4")} onClick={() => ws.go("git")} />
          <Action icon={<TerminalSquare size={14} />} label="Terminal" keys={shortcut("5")} onClick={() => ws.go("terminal")} />
          <Action icon={<Network size={14} />} label="Ask about the code" onClick={() => ws.go("intel")} />
          <Action icon={<Workflow size={14} />} label="New agent task" onClick={() => ws.go("agents")} />
          <Action icon={reporting ? <Loader2 size={14} className="animate-spin" /> : <FileText size={14} />} label="Share a report" onClick={() => void report()} />
        </nav>
        {notice && <div role="status" className="selectable mt-3 text-xs text-ok">{notice}</div>}
        <div className="mt-3 empty:hidden"><ErrorBanner message={error} /></div>

        <div className="mt-6 grid grid-cols-[minmax(0,1fr)_20rem] gap-10 max-[1100px]:grid-cols-1">
          <div className="min-w-0 space-y-8">
            {/* Repository */}
            <section aria-label="Repository">
              <SectionTitle actions={g?.is_repo ? <button className="text-xs text-dim hover:text-ink" onClick={() => ws.go("git")}>Open Git</button> : undefined}>Repository</SectionTitle>
              {git.loading && !git.data ? <Spinner label="Reading Git status…" /> : git.error ? <ErrorBanner message={git.error} /> : !g ? (
                <p className="text-sm text-dim">The project folder is missing, so its repository cannot be read.</p>
              ) : !g.is_repo ? (
                <p className="text-sm text-dim">Not a Git repository. Agents and the Git screen need one; run <code className="font-mono text-xs">git init</code> in the Terminal to create it.</p>
              ) : (
                <div className="space-y-3">
                  <div data-testid="repo-state" className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                    <span className="flex items-center gap-1.5 font-mono text-md"><GitBranch size={15} className="text-faint" />{g.branch ?? "detached HEAD"}</span>
                    {g.upstream ? (
                      <span className="flex items-center gap-2 text-xs text-dim">
                        tracking <span className="font-mono">{g.upstream}</span>
                        {g.ahead > 0 && <span className="flex items-center text-warn"><ArrowUp size={11} />{g.ahead} to push</span>}
                        {g.behind > 0 && <span className="flex items-center text-info"><ArrowDown size={11} />{g.behind} to pull</span>}
                        {g.ahead === 0 && g.behind === 0 && <span>up to date</span>}
                      </span>
                    ) : <span className="text-xs text-faint">no upstream branch</span>}
                  </div>
                  {conflicts.length > 0 && (
                    <div className="flex items-center gap-2 text-sm text-danger"><AlertTriangle size={14} /> {conflicts.length} file{conflicts.length === 1 ? " has" : "s have"} merge conflicts. Resolve them before committing.</div>
                  )}
                  {changes.length === 0 ? (
                    <p className="text-sm text-dim">Working tree clean. Nothing to commit.</p>
                  ) : (
                    <div>
                      <div className="mb-1.5 text-xs text-dim">
                        {changes.length} uncommitted change{changes.length === 1 ? "" : "s"}{staged.length ? ` · ${staged.length} staged` : ""}
                      </div>
                      <ul className="divide-y divide-line/60 border-y border-line/60">
                        {changes.slice(0, 8).map((f) => {
                          const c = changeCode(f);
                          return (
                            <li key={f.path}>
                              <button className="flex w-full items-center gap-3 px-1 py-1.5 text-left hover:bg-raised/50" onClick={() => ws.go("git")} title={`${c.label} — open in Git`}>
                                <span className={clsx("w-3 font-mono text-xs font-semibold", c.cls)}>{c.code}</span>
                                <span className="truncate font-mono text-xs">{f.path}</span>
                              </button>
                            </li>
                          );
                        })}
                      </ul>
                      {changes.length > 8 && <button className="mt-1.5 text-xs text-dim hover:text-ink" onClick={() => ws.go("git")}>and {changes.length - 8} more in Git</button>}
                    </div>
                  )}
                </div>
              )}
            </section>

            {/* Agents for this project, only when there is something to see */}
            {attention.length > 0 && (
              <section aria-label="Agent tasks">
                <SectionTitle actions={<button className="text-xs text-dim hover:text-ink" onClick={() => ws.go("agents")}>Open Agents</button>}>Agent tasks</SectionTitle>
                <ul className="divide-y divide-line/60 border-y border-line/60">
                  {attention.map((r) => (
                    <li key={r.id}>
                      <button className="flex w-full items-center gap-3 px-1 py-2 text-left hover:bg-raised/50" onClick={() => ws.go("agents")}>
                        <Status kind={r.pending_approvals > 0 ? "waiting" : runStatus[r.status]} label={r.pending_approvals > 0 ? "Needs approval" : undefined} className="w-32" />
                        <span className="min-w-0 flex-1 truncate text-sm">{r.title}</span>
                        <span className="shrink-0 text-2xs text-faint">{timeAgo(r.updated_at)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {/* Findings from the last analysis */}
            <section aria-label="Findings">
              <SectionTitle actions={findings.length ? <button className="text-xs text-dim hover:text-ink" onClick={() => ws.go("projects")}>All {findings.length}</button> : undefined}>
                Findings{serious.length ? ` · ${serious.length} to look at` : ""}
              </SectionTitle>
              {!a ? <p className="text-sm text-dim">Not analysed yet. Re-analyse to check for secrets, committed .env files, oversized files and missing tests or CI.</p>
                : findings.length === 0 ? <p className="text-sm text-dim">The last analysis found nothing to report.</p> : (
                  <ul className="space-y-1">
                    {findings.slice(0, 6).map((f, i) => (
                      <li key={i} className="flex items-baseline gap-3 text-sm">
                        <span className={clsx("w-14 shrink-0 text-2xs font-semibold uppercase", severityCls[f.severity])}>{f.severity}</span>
                        <span className="min-w-0 flex-1">{f.message}</span>
                        {f.file && (
                          <button className="shrink-0 truncate font-mono text-2xs text-faint hover:text-ink" onClick={() => ws.peek({ projectId: project.id, path: f.file!, line: f.line ?? undefined })}>
                            {f.file}{f.line ? `:${f.line}` : ""}
                          </button>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
            </section>
          </div>

          <aside className="min-w-0 space-y-8">
            <section aria-label="Recent activity">
              <SectionTitle actions={<button className="text-xs text-dim hover:text-ink" onClick={() => ws.go("activity")}>All activity</button>}>Recent in this project</SectionTitle>
              {activity.error ? <ErrorBanner message={activity.error} /> : recent.length === 0 ? (
                <p className="text-sm text-dim">Nothing yet. Analyses, task runs, agent tasks and terminals in this project show up here.</p>
              ) : (
                <ol data-testid="recent-activity" className="relative space-y-2.5 border-l border-line pl-4">
                  {recent.map((e) => (
                    <li key={e.id} className="relative">
                      <span className="absolute top-[7px] -left-[19px] size-1.5 rounded-full bg-line-strong" />
                      <div className="text-sm leading-5">{e.message}</div>
                      <div className="text-2xs text-faint">{e.kind} · {timeAgo(e.at)}</div>
                    </li>
                  ))}
                </ol>
              )}
            </section>

            {ws.projects.length > 1 && (
              <section aria-label="Other projects">
                <SectionTitle>Other projects</SectionTitle>
                <ul className="space-y-0.5">
                  {ws.projects.filter((p) => p.id !== project.id).map((p) => (
                    <li key={p.id}>
                      <button className="flex w-full items-baseline gap-2 rounded-md px-1.5 py-1 text-left hover:bg-raised" onClick={() => ws.selectProject(p.id)}>
                        <span className="truncate text-sm">{p.name}</span>
                        <span className="ml-auto shrink-0 text-2xs text-faint">{p.exists ? (p.analysis ? primaryLanguage(p.analysis.languages) : "not analysed") : "missing"}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </aside>
        </div>
      </div>
    </div>
  );
}
