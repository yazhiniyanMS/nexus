import clsx from "clsx";
import { File as FileIcon, Search as SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import type { Workspace } from "../App";
import { FilePreview } from "../components/FilePreview";
import { Button, Empty, ErrorBanner, Input, PageHeader, Select } from "../components/ui";
import { api, type SearchMatch, type SearchResult } from "../lib/api";
import { errorText } from "../lib/util";

function Toggle({ on, onClick, title, children }: { on: boolean; onClick: () => void; title: string; children: string }) {
  return (
    <button
      title={title}
      aria-pressed={on}
      onClick={onClick}
      className={clsx("h-8 rounded-md border px-2.5 font-mono text-xs", on ? "border-accent/60 bg-accent/10 text-accent" : "border-line text-dim hover:text-ink")}
    >
      {children}
    </button>
  );
}

export default function SearchView({ ws }: { ws: Workspace }) {
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [scope, setScope] = useState("all");
  const [result, setResult] = useState<SearchResult>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [open, setOpen] = useState<SearchMatch | null>(null);

  const run = async () => {
    if (!query.trim()) return;
    setBusy(true);
    setError(undefined);
    try {
      setResult(await api.search(query, regex, caseSensitive, scope === "all" ? null : [scope]));
      setOpen(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const groups = useMemo(() => {
    const map = new Map<string, SearchMatch[]>();
    for (const m of result?.matches ?? []) {
      const key = `${m.project_id}\u0000${m.path}`;
      map.set(key, [...(map.get(key) ?? []), m]);
    }
    return [...map.values()];
  }, [result]);

  return (
    <>
      <PageHeader title="Search" subtitle="Find files and text across every imported project" />
      <form
        className="flex shrink-0 items-center gap-2 border-b border-line p-3"
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <Input autoFocus placeholder="Search file names and contents…" value={query} onChange={(e) => setQuery(e.target.value)} className="flex-1 font-mono" />
        <Toggle on={caseSensitive} onClick={() => setCaseSensitive(!caseSensitive)} title="Match case">Aa</Toggle>
        <Toggle on={regex} onClick={() => setRegex(!regex)} title="Regular expression">.*</Toggle>
        <Select className="w-48!" value={scope} onChange={(e) => setScope(e.target.value)} aria-label="Scope">
          <option value="all">All projects</option>
          {ws.projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </Select>
        <Button type="submit" variant="primary" busy={busy}><SearchIcon size={14} /> Search</Button>
      </form>
      <div className="empty:hidden p-3 pb-0"><ErrorBanner message={error} /></div>
      {!result ? (
        <Empty icon={<SearchIcon size={28} />} title={ws.projects.length ? "Search your projects" : "Import a project to search it"}>
          Search respects .gitignore and skips generated folders such as node_modules and target.
        </Empty>
      ) : (
        <div className="flex min-h-0 flex-1">
          <div className={clsx("overflow-auto", open ? "w-[45%] shrink-0 border-r border-line" : "flex-1")}>
            <div className="px-4 py-2 text-xs text-dim">
              {result.matches.filter((m) => m.kind === "content").length} matching lines and {result.matches.filter((m) => m.kind === "file").length} file names in {groups.length} files · {result.files_scanned} files scanned
              {result.truncated && " · showing the first 500 matches"}
              {result.files_skipped > 0 && ` · ${result.files_skipped} very large files skipped`}
            </div>
            {groups.map((g) => (
              <div key={g[0].project_id + g[0].path} className="border-t border-line">
                <div className="flex items-baseline gap-2 bg-panel px-4 py-1.5">
                  <span className="truncate font-mono text-xs">{g[0].path}</span>
                  <span className="shrink-0 text-xs text-faint">{g[0].project_name}</span>
                </div>
                {g.map((m) => (
                  <button
                    key={m.kind + m.line}
                    onClick={() => setOpen(m)}
                    className={clsx("flex w-full gap-3 px-4 py-1 text-left font-mono text-xs hover:bg-raised", open === m && "bg-raised")}
                  >
                    {m.kind === "file" ? (
                      <>
                        <span className="flex w-10 shrink-0 justify-end text-accent"><FileIcon size={12} /></span>
                        <span className="font-sans text-accent">File name matches</span>
                      </>
                    ) : (
                      <>
                        <span className="w-10 shrink-0 text-right text-faint tabular-nums">{m.line}</span>
                        <span className="truncate text-dim">{m.text}</span>
                      </>
                    )}
                  </button>
                ))}
              </div>
            ))}
          </div>
          {open && (
            <div className="min-w-0 flex-1">
              <FilePreview projectId={open.project_id} path={open.path} line={open.line || undefined} />
            </div>
          )}
        </div>
      )}
    </>
  );
}
