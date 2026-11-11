import clsx from "clsx";
import { FolderGit2, FolderPlus, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import type { Workspace } from "../App";
import { FilePreview } from "../components/FilePreview";
import { FileTree } from "../components/FileTree";
import { Badge, Button, Card, Empty, ErrorBanner, PageHeader, Tabs } from "../components/ui";
import { api, type Analysis, type Issue, type Project } from "../lib/api";
import { confirmAction, importProject } from "../lib/importProject";
import { colorFor, errorText, formatBytes, formatNumber, timeAgo } from "../lib/util";

type Tab = "overview" | "dependencies" | "findings" | "files";
const severityTone = { high: "red", medium: "amber", low: "blue", info: "neutral" } as const;

function Overview({ a }: { a: Analysis }) {
  const total = Math.max(1, a.languages.reduce((n, l) => n + l.lines, 0));
  return (
    <div className="space-y-4 p-5">
      <Card className="p-4">
        <div className="mb-1.5 text-xs font-medium text-dim">Summary</div>
        <p className="selectable text-[13.5px] leading-relaxed">{a.summary}</p>
        {a.frameworks.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-1.5">
            {a.frameworks.map((f) => <Badge key={f} tone="green">{f}</Badge>)}
          </div>
        )}
      </Card>
      <div className="grid grid-cols-3 gap-3">
        {[["Files", formatNumber(a.file_count)], ["Lines", formatNumber(a.total_lines)], ["Size", formatBytes(a.total_bytes)]].map(([k, v]) => (
          <Card key={k} className="p-4">
            <div className="text-xs text-dim">{k}</div>
            <div className="mt-1 text-xl font-semibold tabular-nums">{v}</div>
          </Card>
        ))}
      </div>
      <div className="grid grid-cols-2 gap-4">
        <Card className="p-4">
          <div className="mb-3 text-xs font-medium text-dim">Languages</div>
          <div className="mb-3 flex h-2 overflow-hidden rounded-full bg-raised">
            {a.languages.map((l) => (
              <div key={l.name} title={l.name} style={{ width: `${(l.lines / total) * 100}%`, background: colorFor(l.name) }} />
            ))}
          </div>
          <ul className="space-y-1.5">
            {a.languages.slice(0, 12).map((l) => (
              <li key={l.name} className="flex items-center gap-2 text-[13px]">
                <span className="size-2 rounded-full" style={{ background: colorFor(l.name) }} />
                <span className="flex-1">{l.name}</span>
                <span className="text-xs text-faint tabular-nums">{formatNumber(l.files)} files</span>
                <span className="w-24 text-right text-xs text-dim tabular-nums">{formatNumber(l.lines)} lines</span>
              </li>
            ))}
          </ul>
        </Card>
        <Card className="p-4">
          <div className="mb-3 text-xs font-medium text-dim">Structure</div>
          <ul className="space-y-1.5">
            {a.top_dirs.map((d) => (
              <li key={d.name} className="flex items-center gap-2 text-[13px]">
                <span className="flex-1 truncate font-mono text-xs">{d.name}</span>
                <span className="text-xs text-dim tabular-nums">{formatNumber(d.files)} files</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>
    </div>
  );
}

function Dependencies({ a }: { a: Analysis }) {
  if (!a.dependencies.length)
    return <Empty title="No dependency manifests found">NEXUS reads package.json, Cargo.toml, requirements.txt, pyproject.toml and go.mod.</Empty>;
  return (
    <div className="space-y-4 p-5">
      {a.dependencies.map((g) => (
        <Card key={g.manifest}>
          <div className="flex items-center gap-2 border-b border-line px-4 py-2.5">
            <span className="font-mono text-xs">{g.manifest}</span>
            <Badge>{g.ecosystem}</Badge>
            <span className="ml-auto text-xs text-faint">{g.deps.length} dependencies</span>
          </div>
          <div className="selectable grid grid-cols-2 gap-x-6 gap-y-1 p-4 xl:grid-cols-3">
            {g.deps.map((d) => (
              <div key={d.name + d.dev} className="flex items-baseline gap-2 text-[13px]">
                <span className="truncate">{d.name}</span>
                {d.dev && <span className="text-[10px] text-faint uppercase">dev</span>}
                <span className="ml-auto shrink-0 font-mono text-xs text-dim">{d.version}</span>
              </div>
            ))}
          </div>
        </Card>
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
            <div className="selectable text-[13px] break-words">{i.message}</div>
            {i.file && (
              <button className="mt-0.5 font-mono text-xs text-sky-300 hover:underline" onClick={() => onOpen(i.file!, i.line ?? undefined)}>
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
  const [tab, setTab] = useState<Tab>("overview");
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
      <PageHeader
        title={project.name}
        subtitle={<span className="selectable">{project.path} · analysed {timeAgo(project.analyzed_at)}</span>}
        actions={
          <>
            <Button onClick={() => run(() => api.analyzeProject(project.id))} busy={busy} disabled={!project.exists}>
              <RefreshCw size={13} /> Re-analyse
            </Button>
            <Button
              variant="ghost"
              title="Remove from NEXUS (files are not deleted)"
              onClick={async () => {
                if (await confirmAction(`Remove “${project.name}” from NEXUS? Your files on disk are not touched.`))
                  await run(() => api.removeProject(project.id));
              }}
            >
              <Trash2 size={14} />
            </Button>
          </>
        }
      />
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "overview", label: "Overview" },
          { id: "dependencies", label: "Dependencies", count: a?.dependencies.reduce((n, g) => n + g.deps.length, 0) },
          { id: "findings", label: "Findings", count: a?.issues.length },
          { id: "files", label: "Files" },
        ]}
      />
      <div className="empty:hidden p-4 pb-0"><ErrorBanner message={error} /></div>
      {!project.exists ? (
        <Empty title="Folder not found">This project's folder was moved or deleted. Remove it and import it again from its new location.</Empty>
      ) : tab === "files" ? (
        <div className="flex min-h-0 flex-1">
          <div className="w-64 shrink-0 overflow-auto border-r border-line">
            <FileTree key={project.id} projectId={project.id} selected={file?.path ?? null} onOpen={(p) => setFile({ path: p })} />
          </div>
          <div className="min-w-0 flex-1">
            {file ? <FilePreview projectId={project.id} path={file.path} line={file.line} /> : <Empty title="Select a file to preview" />}
          </div>
        </div>
      ) : !a ? (
        <Empty title="Not analysed yet">Run an analysis to see languages, dependencies and findings.</Empty>
      ) : (
        <div className="flex-1 overflow-auto">
          {tab === "overview" && <Overview a={a} />}
          {tab === "dependencies" && <Dependencies a={a} />}
          {tab === "findings" && <Findings issues={a.issues} onOpen={openFile} />}
        </div>
      )}
    </div>
  );
}

export default function ProjectsView({ ws }: { ws: Workspace }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

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
      <aside className="flex w-60 shrink-0 flex-col border-r border-line">
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-line px-4">
          <span className="text-[13px] font-semibold">Projects</span>
          <Button size="sm" variant="primary" onClick={onImport} busy={busy}>
            <FolderPlus size={13} /> Import
          </Button>
        </div>
        <div className="empty:hidden p-2"><ErrorBanner message={error} /></div>
        <div className="flex-1 overflow-auto p-2">
          {ws.projects.map((p) => (
            <button
              key={p.id}
              onClick={() => ws.selectProject(p.id)}
              className={clsx("mb-0.5 block w-full rounded-md px-2.5 py-2 text-left", p.id === ws.projectId ? "bg-raised" : "hover:bg-raised/60")}
            >
              <div className="truncate text-[13px] font-medium">{p.name}</div>
              <div className="truncate text-xs text-faint">
                {!p.exists ? "folder missing" : p.analysis ? `${formatNumber(p.analysis.file_count)} files · ${p.analysis.languages[0]?.name ?? "no code"}` : "not analysed"}
              </div>
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
