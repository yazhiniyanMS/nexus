// Git: review changes, stage them, commit them; read history and branches.
// Only the index and new commits are ever written. Files on disk are never changed or discarded.
import clsx from "clsx";
import { AlertTriangle, ArrowDown, ArrowUp, GitBranch, GitCommitHorizontal, Minus, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Workspace } from "../App";
import { Badge, Button, DiffView, Empty, ErrorBanner, IconButton, Kbd, Spinner, Tabs, ViewHeader } from "../components/ui";
import { api, type FileStatus } from "../lib/api";
import { isMac, modPressed } from "../lib/platform";
import { errorText, timeAgo, useAsync } from "../lib/util";
import { changeCode } from "./Dashboard";

type Tab = "changes" | "history" | "branches";
type Group = "conflicts" | "staged" | "changed" | "untracked";
interface Entry { key: string; group: Group; file: FileStatus }

const GROUPS: { id: Group; title: string; staged: boolean }[] = [
  { id: "conflicts", title: "Merge conflicts", staged: false },
  { id: "staged", title: "Staged", staged: true },
  { id: "changed", title: "Changes", staged: false },
  { id: "untracked", title: "Untracked", staged: false },
];

function DiffPane({ load, emptyTitle, emptyHint }: { load: (() => Promise<string>) | null; emptyTitle: string; emptyHint?: string }) {
  const [body, setBody] = useState<{ text?: string; error?: string }>({});
  useEffect(() => {
    if (!load) return setBody({});
    let live = true;
    setBody({});
    load().then((text) => live && setBody({ text })).catch((e) => live && setBody({ error: errorText(e) }));
    return () => {
      live = false;
    };
  }, [load]);
  if (!load) return <Empty title={emptyTitle}>{emptyHint}</Empty>;
  if (body.error) return <div className="p-4"><ErrorBanner message={body.error} /></div>;
  if (body.text === undefined) return <Spinner label="Loading diff…" />;
  if (body.text.trim() === "") return <Empty title="No textual changes">The file may be binary, or only its permissions changed.</Empty>;
  return <DiffView text={body.text} />;
}

export default function GitView({ ws }: { ws: Workspace }) {
  const id = ws.projectId;
  const [tab, setTab] = useState<Tab>("changes");
  const [selected, setSelected] = useState<string | null>(null);
  const [commitSel, setCommitSel] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const listRef = useRef<HTMLDivElement>(null);
  const status = useAsync(() => (id ? api.gitStatus(id) : Promise.resolve(undefined)), [id]);
  const isRepo = status.data?.is_repo ?? false;
  const log = useAsync(() => (id && isRepo ? api.gitLog(id) : Promise.resolve([])), [id, isRepo]);
  const branches = useAsync(() => (id && isRepo ? api.gitBranches(id) : Promise.resolve([])), [id, isRepo]);

  // Changes made outside NEXUS (an editor, the terminal) show up when you come back to the window.
  useEffect(() => {
    const onFocus = () => {
      setNotice(undefined);
      void status.reload();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  useEffect(() => {
    setSelected(null);
    setCommitSel(null);
    setError(undefined);
    setNotice(undefined);
  }, [id]);

  const s = status.data;
  const entries = useMemo<Entry[]>(() => {
    const files = s?.files ?? [];
    const out: Entry[] = [];
    for (const f of files.filter((f) => f.conflicted)) out.push({ key: `conflicts:${f.path}`, group: "conflicts", file: f });
    for (const f of files.filter((f) => !f.untracked && !f.conflicted && f.index !== ".")) out.push({ key: `staged:${f.path}`, group: "staged", file: f });
    for (const f of files.filter((f) => !f.untracked && !f.conflicted && f.worktree !== ".")) out.push({ key: `changed:${f.path}`, group: "changed", file: f });
    for (const f of files.filter((f) => f.untracked)) out.push({ key: `untracked:${f.path}`, group: "untracked", file: f });
    return out;
  }, [s]);
  const stagedCount = entries.filter((e) => e.group === "staged").length;
  const conflictCount = entries.filter((e) => e.group === "conflicts").length;
  const current = entries.find((e) => e.key === selected) ?? null;

  const loadDiff = useMemo(() => {
    if (!current || !id) return null;
    const f = current.file;
    return () => api.gitDiff(id, f.path, current.group === "staged", f.untracked);
  }, [current, id]);
  const loadCommit = useMemo(() => (commitSel && id ? () => api.gitShow(id, commitSel) : null), [commitSel, id]);

  const run = useCallback(async (what: string, fn: () => Promise<unknown>) => {
    setBusy(what);
    setError(undefined);
    setNotice(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      await status.reload();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  /** Stage or unstage paths. Only the index changes; files on disk are never modified. */
  const setStaged = (paths: string[], stage: boolean) => run(stage ? "stage" : "unstage", () => (stage ? api.gitStage(id!, paths) : api.gitUnstage(id!, paths)));

  const commit = async () => {
    if (!id || !message.trim() || stagedCount === 0 || conflictCount > 0) return;
    setBusy("commit");
    setError(undefined);
    setNotice(undefined);
    try {
      const done = await api.gitCommit(id, message);
      setMessage("");
      setSelected(null);
      setNotice(`Committed ${done.short} “${done.subject}”`);
      void log.reload();
      void branches.reload();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      await status.reload();
    }
  };

  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowDown", "ArrowUp"].includes(e.key) || entries.length === 0) return;
    e.preventDefault();
    const i = entries.findIndex((x) => x.key === selected);
    const next = e.key === "ArrowDown" ? Math.min(entries.length - 1, i + 1) : Math.max(0, i === -1 ? 0 : i - 1);
    setSelected(entries[next].key);
    listRef.current?.querySelector(`[data-entry="${CSS.escape(entries[next].key)}"]`)?.scrollIntoView({ block: "nearest" });
  };

  if (!id) {
    return (
      <>
        <ViewHeader title="Git" />
        <Empty icon={<GitBranch size={28} />} title="Import a project to see its repository" />
      </>
    );
  }

  const commitBlocked = !message.trim() ? "Write a commit message" : conflictCount > 0 ? "Resolve conflicts first" : stagedCount === 0 ? "Stage changes to commit" : undefined;

  return (
    <>
      <ViewHeader
        title="Git"
        context={
          s?.is_repo ? (
            <span data-testid="git-branch" className="flex items-center gap-2">
              <GitBranch size={12} className="text-faint" />
              <span className="font-mono text-ink">{s.branch ?? "detached HEAD"}</span>
              {s.upstream ? <span className="text-faint">→ {s.upstream}</span> : <span className="text-faint">no upstream</span>}
              {s.ahead > 0 && <span className="flex items-center text-warn" title={`${s.ahead} commit${s.ahead === 1 ? "" : "s"} not pushed`}><ArrowUp size={11} />{s.ahead}</span>}
              {s.behind > 0 && <span className="flex items-center text-info" title={`${s.behind} commit${s.behind === 1 ? "" : "s"} to pull`}><ArrowDown size={11} />{s.behind}</span>}
              {s.merging && <Badge tone="amber">merging</Badge>}
              {s.rebasing && <Badge tone="amber">rebasing</Badge>}
            </span>
          ) : <span>{ws.project?.name}</span>
        }
        actions={<IconButton label="Refresh" onClick={() => { void status.reload(); void log.reload(); void branches.reload(); }}><RefreshCw size={14} className={clsx(status.loading && "animate-spin")} /></IconButton>}
      />
      {status.loading && !s ? (
        <Spinner label="Reading the repository…" />
      ) : status.error ? (
        <div className="p-4"><ErrorBanner message={status.error} /></div>
      ) : !s?.is_repo ? (
        <Empty icon={<GitBranch size={28} />} title="Not a Git repository">
          This folder has no Git history. Run <code className="font-mono text-xs">git init</code> in the Terminal to start tracking it; NEXUS does not create repositories for you.
          <div className="mt-3"><Button onClick={() => ws.go("terminal")}>Open Terminal</Button></div>
        </Empty>
      ) : (
        <>
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { id: "changes", label: "Changes", count: entries.length },
              { id: "history", label: "History", count: log.data?.length },
              { id: "branches", label: "Branches", count: branches.data?.length },
            ]}
          />

          {tab === "changes" && (
            <div className="flex min-h-0 flex-1">
              <div className="flex w-[22rem] shrink-0 flex-col border-r border-line">
                {(s.merging || s.rebasing || conflictCount > 0) && (
                  <div data-testid="git-conflict-banner" className="flex gap-2 border-b border-line bg-warn/5 px-3 py-2 text-xs leading-relaxed text-warn">
                    <AlertTriangle size={14} className="mt-0.5 shrink-0" />
                    <span>
                      {s.rebasing ? "A rebase is in progress." : s.merging ? "A merge is in progress." : ""}{" "}
                      {conflictCount > 0
                        ? `Fix the ${conflictCount} conflicted file${conflictCount === 1 ? "" : "s"} in your editor, then stage ${conflictCount === 1 ? "it" : "them"} to mark resolved.`
                        : "All conflicts are resolved. Commit to finish, or abort with git in the Terminal."}
                    </span>
                  </div>
                )}
                <div ref={listRef} tabIndex={0} onKeyDown={onListKey} aria-label="Changed files" className="min-h-0 flex-1 overflow-auto py-1 outline-none focus-visible:shadow-[inset_2px_0_0_var(--color-lav)]">
                  {entries.length === 0 ? (
                    <div className="px-4 py-8 text-center">
                      <div className="text-sm font-medium">Working tree clean</div>
                      <div className="mt-1 text-xs text-dim">{s.has_commits ? "Nothing to commit. Edits you make appear here." : "No commits yet. Add files to the folder, then stage and commit them here."}</div>
                    </div>
                  ) : GROUPS.map((g) => {
                    const list = entries.filter((e) => e.group === g.id);
                    if (list.length === 0) return null;
                    return (
                      <div key={g.id} data-group={g.id} className="mb-1">
                        <div className="flex h-7 items-center px-3 text-xs font-semibold text-dim">
                          <span className={clsx(g.id === "conflicts" && "text-danger")}>{g.title}</span>
                          <span className="ml-1.5 font-normal text-faint">{list.length}</span>
                          <button
                            className="ml-auto text-2xs font-medium text-faint hover:text-ink disabled:opacity-40"
                            disabled={!!busy}
                            onClick={() => void setStaged(list.map((e) => e.file.path), !g.staged)}
                          >
                            {g.staged ? "Unstage all" : g.id === "conflicts" ? "Mark all resolved" : "Stage all"}
                          </button>
                        </div>
                        {list.map((e) => {
                          const c = changeCode(e.file);
                          return (
                            <div key={e.key} data-entry={e.key} className={clsx("group flex h-7 items-center pr-1.5", selected === e.key ? "bg-raised" : "hover:bg-raised/50")}>
                              <button className="flex min-w-0 flex-1 items-center gap-2 pl-3 text-left" onClick={() => setSelected(e.key)} title={`${e.file.path} — ${c.label}`}>
                                <span className={clsx("w-3 shrink-0 text-center font-mono text-xs font-semibold", c.cls)}>{c.code}</span>
                                <span className="truncate font-mono text-xs">
                                  <span className="text-ink">{e.file.path.split("/").pop()}</span>
                                  {e.file.path.includes("/") && <span className="ml-1.5 text-faint">{e.file.path.slice(0, e.file.path.lastIndexOf("/"))}</span>}
                                </span>
                              </button>
                              <IconButton
                                label={g.staged ? `Unstage ${e.file.path}` : g.id === "conflicts" ? `Mark ${e.file.path} resolved` : `Stage ${e.file.path}`}
                                className="size-6 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                                disabled={!!busy}
                                onClick={() => void setStaged([e.file.path], !g.staged)}
                              >
                                {g.staged ? <Minus size={13} /> : <Plus size={13} />}
                              </IconButton>
                            </div>
                          );
                        })}
                      </div>
                    );
                  })}
                </div>

                {/* Commit */}
                <form className="shrink-0 space-y-2 border-t border-line p-3" onSubmit={(e) => { e.preventDefault(); void commit(); }}>
                  <textarea
                    aria-label="Commit message"
                    value={message}
                    onChange={(e) => setMessage(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && modPressed(e)) {
                        e.preventDefault();
                        void commit();
                      }
                    }}
                    rows={3}
                    placeholder={stagedCount ? `Message for ${stagedCount} staged file${stagedCount === 1 ? "" : "s"}` : "Commit message"}
                    className="w-full resize-none rounded-md border border-line bg-bg p-2 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none"
                  />
                  <div className="flex items-center gap-2">
                    <Button type="submit" variant="primary" size="sm" busy={busy === "commit"} disabled={!!commitBlocked || (!!busy && busy !== "commit")} title={commitBlocked}>
                      <GitCommitHorizontal size={13} /> Commit
                    </Button>
                    <Kbd>{isMac ? "⌘↵" : "Ctrl+↵"}</Kbd>
                    <span className="ml-auto truncate text-2xs text-faint">{commitBlocked ?? `${stagedCount} staged`}</span>
                  </div>
                  {notice && <div role="status" data-testid="git-notice" className="truncate text-xs text-ok">{notice}</div>}
                  <ErrorBanner message={error} />
                </form>
              </div>
              <div className="selectable min-w-0 flex-1 overflow-auto" data-testid="git-diff">
                <DiffPane
                  load={loadDiff}
                  emptyTitle={entries.length ? "Select a file to see its changes" : "No changes"}
                  emptyHint={entries.length ? "Use ↑ and ↓ in the file list to move between files." : undefined}
                />
              </div>
            </div>
          )}

          {tab === "history" && (
            <div className="flex min-h-0 flex-1">
              <div className="w-[26rem] shrink-0 overflow-auto border-r border-line">
                {log.error ? <div className="p-3"><ErrorBanner message={log.error} /></div> : log.loading && !log.data ? <Spinner /> : log.data?.length === 0 ? (
                  <Empty title="No commits yet">Your first commit will appear here.</Empty>
                ) : log.data?.map((c) => (
                  <button
                    key={c.hash}
                    data-commit={c.short}
                    onClick={() => setCommitSel(c.hash)}
                    className={clsx("block w-full border-b border-line/60 px-3 py-2 text-left", commitSel === c.hash ? "bg-raised" : "hover:bg-raised/50")}
                  >
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm">{c.subject}</span>
                      {c.refs && c.refs.split(", ").slice(0, 2).map((r) => <Badge key={r} tone="blue">{r.replace("HEAD -> ", "")}</Badge>)}
                    </div>
                    <div className="mt-0.5 flex gap-2 text-2xs text-faint">
                      <span className="font-mono">{c.short}</span>
                      <span className="truncate">{c.author}</span>
                      <span className="ml-auto shrink-0">{timeAgo(c.date)}</span>
                    </div>
                  </button>
                ))}
              </div>
              <div className="selectable min-w-0 flex-1 overflow-auto"><DiffPane load={loadCommit} emptyTitle="Select a commit to see what it changed" /></div>
            </div>
          )}

          {tab === "branches" && (
            <div className="min-h-0 flex-1 overflow-auto">
              {branches.error ? <div className="p-3"><ErrorBanner message={branches.error} /></div> : (
                <table className="w-full text-sm">
                  <thead className="text-left text-xs text-faint">
                    <tr className="border-b border-line"><th className="px-4 py-2 font-medium">Branch</th><th className="font-medium">Last commit</th><th className="font-medium">Tracks</th><th className="px-4 text-right font-medium">Updated</th></tr>
                  </thead>
                  <tbody className="divide-y divide-line/60">
                    {branches.data?.map((b) => (
                      <tr key={b.name} data-branch={b.name} className={clsx(b.current && "bg-raised/40")}>
                        <td className="px-4 py-2">
                          <span className="flex items-center gap-2">
                            <span className={clsx("font-mono text-xs", b.current ? "font-semibold text-ink" : "text-dim")}>{b.name}</span>
                            {b.current && <Badge tone="green">checked out</Badge>}
                            {b.remote && <Badge>remote</Badge>}
                          </span>
                        </td>
                        <td className="max-w-0 truncate py-2 pr-4 text-dim"><span className="mr-2 font-mono text-2xs text-faint">{b.commit}</span>{b.subject}</td>
                        <td className="py-2 font-mono text-2xs text-faint">{b.upstream ?? "—"}</td>
                        <td className="px-4 py-2 text-right text-2xs whitespace-nowrap text-faint">{timeAgo(b.date)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <p className="px-4 py-3 text-2xs text-faint">Switching, creating and deleting branches is done in the Terminal; NEXUS never changes which branch is checked out.</p>
            </div>
          )}
        </>
      )}
    </>
  );
}
