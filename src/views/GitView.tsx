import clsx from "clsx";
import { ArrowDown, ArrowUp, GitBranch, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import type { Workspace } from "../App";
import { ProjectPicker } from "../components/ProjectPicker";
import { Badge, Button, DiffView, Empty, ErrorBanner, PageHeader, Spinner, Tabs } from "../components/ui";
import { api, type FileStatus } from "../lib/api";
import { errorText, timeAgo, useAsync } from "../lib/util";

type Tab = "changes" | "history" | "branches";
type Selection = { title: string; load: () => Promise<string> };

const statusLabel: Record<string, string> = { M: "modified", A: "added", D: "deleted", R: "renamed", C: "copied", T: "type", U: "conflict", "?": "untracked" };

function FileRow({ f, letter, active, onClick }: { f: FileStatus; letter: string; active: boolean; onClick: () => void }) {
  return (
    <button onClick={onClick} className={clsx("flex w-full items-center gap-2 px-4 py-1.5 text-left hover:bg-raised", active && "bg-raised")}>
      <span
        title={statusLabel[letter] ?? letter}
        className={clsx("w-4 shrink-0 text-center font-mono text-xs font-bold", letter === "D" ? "text-red-400" : letter === "?" || letter === "A" ? "text-emerald-400" : "text-amber-400")}
      >
        {letter}
      </span>
      <span className="truncate font-mono text-xs">{f.path}</span>
    </button>
  );
}

export default function GitView({ ws }: { ws: Workspace }) {
  const id = ws.projectId;
  const [tab, setTab] = useState<Tab>("changes");
  const [sel, setSel] = useState<Selection | null>(null);
  const [body, setBody] = useState<{ text?: string; error?: string }>({});
  const status = useAsync(() => (id ? api.gitStatus(id) : Promise.resolve(undefined)), [id]);
  const isRepo = status.data?.is_repo ?? false;
  const log = useAsync(() => (id && isRepo ? api.gitLog(id) : Promise.resolve([])), [id, isRepo]);
  const branches = useAsync(() => (id && isRepo ? api.gitBranches(id) : Promise.resolve([])), [id, isRepo]);

  useEffect(() => setSel(null), [id, tab]);
  useEffect(() => {
    if (!sel) return setBody({});
    let live = true;
    setBody({});
    sel.load().then((text) => live && setBody({ text })).catch((e) => live && setBody({ error: errorText(e) }));
    return () => {
      live = false;
    };
  }, [sel]);

  if (!id)
    return (
      <>
        <PageHeader title="Git" />
        <Empty icon={<GitBranch size={28} />} title="Import a project to inspect its repository" />
      </>
    );

  const s = status.data;
  const files = s?.files ?? [];
  const staged = files.filter((f) => !f.untracked && f.index !== ".");
  const unstaged = files.filter((f) => !f.untracked && f.worktree !== ".");
  const untracked = files.filter((f) => f.untracked);
  const refresh = () => {
    void status.reload();
    void log.reload();
    void branches.reload();
  };

  const group = (title: string, list: FileStatus[], letter: (f: FileStatus) => string, isStaged: boolean) =>
    list.length > 0 && (
      <div>
        <div className="bg-panel px-4 py-1.5 text-xs font-semibold text-dim">{title} <span className="text-faint">{list.length}</span></div>
        {list.map((f) => {
          const title = `${f.path}${isStaged ? " (staged)" : ""}`;
          return (
            <FileRow key={title} f={f} letter={letter(f)} active={sel?.title === title} onClick={() => setSel({ title, load: () => api.gitDiff(id, f.path, isStaged, f.untracked) })} />
          );
        })}
      </div>
    );

  return (
    <>
      <PageHeader
        title="Git"
        subtitle={
          s?.is_repo ? (
            <span className="flex items-center gap-2">
              <GitBranch size={12} /> {s.branch ?? "detached"}
              {s.upstream && <span className="text-faint">→ {s.upstream}</span>}
              {s.ahead > 0 && <span className="flex items-center text-emerald-300"><ArrowUp size={11} />{s.ahead}</span>}
              {s.behind > 0 && <span className="flex items-center text-amber-300"><ArrowDown size={11} />{s.behind}</span>}
            </span>
          ) : undefined
        }
        actions={
          <>
            <ProjectPicker ws={ws} />
            <Button onClick={refresh}><RefreshCw size={13} /> Refresh</Button>
          </>
        }
      />
      <div className="empty:hidden p-3 pb-0"><ErrorBanner message={status.error ?? log.error ?? branches.error} /></div>
      {status.loading && !s ? (
        <Spinner />
      ) : !s?.is_repo ? (
        <Empty icon={<GitBranch size={28} />} title="Not a Git repository">
          {status.error ? "Git status could not be read." : "This folder has no Git history. Run git init in it to start tracking changes."}
        </Empty>
      ) : (
        <>
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { id: "changes", label: "Changes", count: files.length },
              { id: "history", label: "History", count: log.data?.length },
              { id: "branches", label: "Branches", count: branches.data?.length },
            ]}
          />
          <div className="flex min-h-0 flex-1">
            <div className={clsx("overflow-auto", tab === "branches" ? "flex-1" : "w-[42%] shrink-0 border-r border-line")}>
              {tab === "changes" &&
                (files.length === 0 ? (
                  <Empty title="Working tree clean">No uncommitted changes.</Empty>
                ) : (
                  <>
                    {group("Staged", staged, (f) => f.index, true)}
                    {group("Changed", unstaged, (f) => (f.conflicted ? "U" : f.worktree), false)}
                    {group("Untracked", untracked, () => "?", false)}
                  </>
                ))}
              {tab === "history" &&
                (log.data?.length === 0 ? (
                  <Empty title="No commits yet" />
                ) : (
                  log.data?.map((c) => (
                    <button
                      key={c.hash}
                      onClick={() => setSel({ title: c.hash, load: () => api.gitShow(id, c.hash) })}
                      className={clsx("block w-full border-b border-line px-4 py-2 text-left hover:bg-raised", sel?.title === c.hash && "bg-raised")}
                    >
                      <div className="flex items-center gap-2">
                        <span className="truncate text-[13px]">{c.subject}</span>
                        {c.refs && c.refs.split(", ").slice(0, 3).map((r) => <Badge key={r} tone="blue">{r.replace("HEAD -> ", "")}</Badge>)}
                      </div>
                      <div className="mt-0.5 flex gap-2 text-xs text-faint">
                        <span className="font-mono">{c.short}</span>
                        <span className="truncate">{c.author}</span>
                        <span className="ml-auto shrink-0">{timeAgo(c.date)}</span>
                      </div>
                    </button>
                  ))
                ))}
              {tab === "branches" && (
                <table className="w-full text-[13px]">
                  <tbody className="divide-y divide-line">
                    {branches.data?.map((b) => (
                      <tr key={b.name}>
                        <td className="px-5 py-2">
                          <span className="flex items-center gap-2">
                            <span className={clsx("font-mono text-xs", b.current && "font-bold text-accent")}>{b.name}</span>
                            {b.current && <Badge tone="green">current</Badge>}
                            {b.remote && <Badge>remote</Badge>}
                          </span>
                        </td>
                        <td className="py-2 font-mono text-xs text-faint">{b.commit}</td>
                        <td className="max-w-0 w-1/2 truncate py-2 text-dim">{b.subject}</td>
                        <td className="py-2 text-xs text-faint">{b.upstream}</td>
                        <td className="px-5 py-2 text-right text-xs text-faint whitespace-nowrap">{timeAgo(b.date)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            {tab !== "branches" && (
              <div className="selectable min-w-0 flex-1 overflow-auto">
                {!sel ? (
                  <Empty title={tab === "changes" ? "Select a file to see its diff" : "Select a commit to see its changes"} />
                ) : body.error ? (
                  <div className="p-4"><ErrorBanner message={body.error} /></div>
                ) : body.text === undefined ? (
                  <Spinner />
                ) : body.text.trim() === "" ? (
                  <Empty title="No textual changes">The file may be binary or only its mode changed.</Empty>
                ) : (
                  <DiffView text={body.text} />
                )}
              </div>
            )}
          </div>
        </>
      )}
    </>
  );
}
