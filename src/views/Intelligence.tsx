import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Copy, Download, FileText, Network, RefreshCw, Search, Sparkles, X } from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from "react";
import type { Workspace } from "../App";
import { FilePreview } from "../components/FilePreview";
import { GraphView } from "../components/GraphView";
import { ModelChoiceControls, ModelChoiceNotice, useModelChoice } from "../components/ModelChoice";
import { Badge, Button, Card, Empty, ErrorBanner, Input, Select, Spinner, Tabs, ViewHeader } from "../components/ui";
import { api, INDEX_PROGRESS, type Answer, type Provider, type Reference, type Source, type SymbolHit } from "../lib/api";
import { mermaidFromGraph } from "../lib/share";
import { errorText, formatNumber, timeAgo, useAsync } from "../lib/util";

const CodeViewer = lazy(() => import("../components/CodeViewer"));
type Tab = "map" | "symbols" | "ask" | "docs";
const KINDS = ["function", "method", "class", "struct", "interface", "trait", "enum", "type", "constant", "macro", "module"];

// ------------------------------------------------------------------ Map

function MapTab({ projectId, ws }: { projectId: string; ws: Workspace }) {
  const [mode, setMode] = useState("module-2");
  const [externals, setExternals] = useState(false);
  // Start at the folder depth that shows the most dependencies while staying readable: a project
  // whose code sits in one deep folder (src-tauri/src/…) looks like a single circle at two levels.
  useEffect(() => {
    let live = true;
    Promise.all([1, 2, 3].map((d) => api.dependencyGraph(projectId, "module", d, false)))
      .then((graphs) => {
        let best = 1;
        graphs.forEach((g, i) => {
          if (g.nodes.length <= 36 && g.edges.length > graphs[best - 1].edges.length) best = i + 1;
        });
        if (live) setMode(`module-${best}`);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [projectId]);
  const [selected, setSelected] = useState<string | null>(null);
  const [copied, setCopied] = useState<string>();
  const [level, depth] = mode === "file" ? (["file", 1] as const) : (["module", Number(mode.split("-")[1])] as const);
  const graph = useAsync(() => api.dependencyGraph(projectId, level, depth, externals), [projectId, mode, externals]);
  useEffect(() => {
    setSelected(null);
    setCopied(undefined);
  }, [projectId, mode, externals]);

  const g = graph.data;
  const node = g?.nodes.find((n) => n.id === selected);
  const out = g?.edges.filter((e) => e.from === selected) ?? [];
  const inn = g?.edges.filter((e) => e.to === selected) ?? [];
  const label = (id: string) => g?.nodes.find((n) => n.id === id)?.label ?? id;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-line px-4 py-2 text-xs text-dim">
        <Select aria-label="Graph level" className="w-64!" value={mode} onChange={(e) => setMode(e.target.value)}>
          <option value="module-1">Architecture map (top-level modules)</option>
          <option value="module-2">Architecture map (two levels)</option>
          <option value="module-3">Architecture map (three levels)</option>
          <option value="file">File dependency graph</option>
        </Select>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" className="accent-accent" checked={externals} onChange={(e) => setExternals(e.target.checked)} />
          External packages
        </label>
        {g && (
          <span className="ml-auto flex items-center gap-3">
            <span>{g.nodes.length} nodes · {g.edges.length} dependencies</span>
            {g.cycles.length > 0 && <span className="flex items-center gap-1 text-danger"><AlertTriangle size={12} /> {g.cycles.length} cycle{g.cycles.length > 1 ? "s" : ""}</span>}
            {g.truncated && <span className="text-warn">showing the most connected nodes</span>}
            {g.nodes.length > 0 && (
              <Button
                size="sm"
                title="Copies the map as a Mermaid diagram, which GitHub shows as a picture in READMEs, issues and pull requests. It contains module names and the imports between them, no source code."
                onClick={async () => {
                  const title = mode === "file" ? "File dependency graph" : `Architecture map, ${depth === 1 ? "top-level modules" : `${depth} folder levels`}`;
                  try {
                    await navigator.clipboard.writeText(mermaidFromGraph(g, title));
                    setCopied("Copied as Mermaid: names only, no code.");
                  } catch (e) {
                    setCopied(`Could not copy: ${errorText(e)}`);
                  }
                }}
              >
                <Copy size={12} /> Copy as Mermaid
              </Button>
            )}
            {copied && <span data-testid="map-copied" className="text-faint">{copied}</span>}
          </span>
        )}
      </div>
      <div className="empty:hidden p-3"><ErrorBanner message={graph.error} /></div>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          {!g ? <Spinner /> : g.nodes.length === 0 ? (
            <Empty icon={<Network size={28} />} title="Nothing to map yet">No Rust, TypeScript, JavaScript, Python or Go source files were indexed in this project.</Empty>
          ) : (
            <GraphView graph={g} selected={selected} onSelect={setSelected} />
          )}
        </div>
        <aside className="w-72 shrink-0 overflow-auto border-l border-line p-4 text-sm" data-testid="graph-details">
          {!node ? (
            <div className="space-y-3 text-xs leading-relaxed text-dim">
              <p>Click a node to see what it depends on and what depends on it. Drag to move nodes, drag the background to pan, scroll to zoom.</p>
              <p>Arrows point from the importer to what it imports. Thicker lines mean more imports. A red outline marks a dependency cycle.</p>
              {g && g.cycles.length > 0 && (
                <div>
                  <div className="mb-1 font-semibold text-danger">Cycles</div>
                  {g.cycles.map((c, i) => <div key={i} className="selectable mb-1 font-mono text-2xs break-all">{c.join(" ↔ ")}</div>)}
                </div>
              )}
            </div>
          ) : (
            <div className="space-y-3">
              <div>
                <div className="selectable font-mono text-xs break-all">{node.id.replace(/^ext:/, "")}</div>
                <div className="mt-1 flex flex-wrap gap-1.5">
                  <Badge tone={node.kind === "external" ? "neutral" : "green"}>{node.kind}</Badge>
                  {node.in_cycle && <Badge tone="red">in a cycle</Badge>}
                </div>
              </div>
              {node.kind !== "external" && (
                <div className="text-xs text-dim">
                  {node.kind === "module" && <>{formatNumber(node.files)} files · </>}{formatNumber(node.lines)} lines
                </div>
              )}
              {node.kind === "file" && <Button size="sm" onClick={() => ws.peek({ projectId, path: node.id })}><FileText size={12} /> Open file</Button>}
              {([["Depends on", out, "to"], ["Used by", inn, "from"]] as const).map(([title, list, key]) => (
                <div key={title}>
                  <div className="mb-1 text-xs font-semibold text-dim">{title} <span className="text-faint">{list.length}</span></div>
                  {list.length === 0 && <div className="text-xs text-faint">Nothing.</div>}
                  {list.map((e) => (
                    <button key={e[key]} onClick={() => setSelected(e[key])} className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left hover:bg-raised">
                      <span className="truncate font-mono text-xs">{label(e[key])}</span>
                      <span className="ml-auto shrink-0 text-2xs text-faint">{e.weight}×</span>
                    </button>
                  ))}
                </div>
              ))}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Symbols

function SymbolsTab({ projectId }: { projectId: string }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("");
  const [picked, setPicked] = useState<SymbolHit | null>(null);
  const [target, setTarget] = useState<{ path: string; line: number } | null>(null);
  const hits = useAsync(() => api.searchSymbols(projectId, query, kind || null, 200), [projectId, query, kind]);
  const refs = useAsync<Reference[]>(() => (picked ? api.findReferences(projectId, picked.name) : Promise.resolve([])), [projectId, picked?.name]);
  useEffect(() => { setPicked(null); setTarget(null); }, [projectId]);

  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex w-[38%] shrink-0 flex-col border-r border-line">
        <div className="flex shrink-0 gap-2 border-b border-line p-3">
          <Input autoFocus placeholder="Find a function, type or constant…" className="font-mono" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Symbol name" />
          <Select aria-label="Kind" className="w-32!" value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="">All kinds</option>
            {KINDS.map((k) => <option key={k}>{k}</option>)}
          </Select>
        </div>
        <ErrorBanner message={hits.error} />
        <div className="flex-1 overflow-auto" data-testid="symbol-results">
          {hits.data?.length === 0 && <Empty title="No symbols found">Try part of a name. Symbols are indexed for Rust, TypeScript, JavaScript, Python and Go.</Empty>}
          {hits.data?.map((s) => (
            <button
              key={`${s.path}:${s.line}:${s.name}`}
              onClick={() => { setPicked(s); setTarget({ path: s.path, line: s.line }); }}
              className={clsx("block w-full border-b border-line/60 px-3 py-1.5 text-left hover:bg-raised", picked === s && "bg-raised")}
            >
              <div className="flex items-center gap-2">
                <span className="truncate font-mono text-sm">{s.name}</span>
                <Badge>{s.kind}</Badge>
              </div>
              <div className="truncate font-mono text-2xs text-faint">{s.path}:{s.line}</div>
            </button>
          ))}
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        {!picked || !target ? (
          <Empty icon={<Search size={26} />} title="Select a symbol">See its definition and every place its name is used.</Empty>
        ) : (
          <>
            <div className="min-h-0 flex-1"><FilePreview projectId={projectId} path={target.path} line={target.line} /></div>
            <div className="h-[38%] shrink-0 overflow-auto border-t border-line" data-testid="references">
              <div className="sticky top-0 bg-panel px-3 py-1.5 text-xs font-semibold text-dim">
                References to <span className="font-mono text-ink">{picked.name}</span> <span className="text-faint">{refs.loading ? "…" : refs.data?.length}</span>
                <span className="ml-2 font-normal text-faint">matched by name</span>
              </div>
              {refs.data?.map((r) => (
                <button
                  key={`${r.path}:${r.line}`}
                  onClick={() => setTarget({ path: r.path, line: r.line })}
                  className={clsx("flex w-full items-baseline gap-3 px-3 py-1 text-left font-mono text-xs hover:bg-raised", target.path === r.path && target.line === r.line && "bg-raised")}
                >
                  <span className="w-56 shrink-0 truncate text-lav">{r.path}:{r.line}</span>
                  {r.is_definition && <Badge tone="green">definition</Badge>}
                  <span className="truncate text-dim">{r.text}</span>
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Ask

/** Render an answer, turning [n] citations into buttons that open the cited source. */
function Cited({ text, sources, onOpen }: { text: string; sources: Source[]; onOpen: (s: Source) => void }) {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(/\[(\d+)\]/g)) {
    const source = sources.find((s) => s.n === Number(m[1]));
    if (!source) continue;
    parts.push(text.slice(last, m.index));
    parts.push(
      <button key={m.index} title={`${source.path}:${source.start_line}`} onClick={() => onOpen(source)} className="mx-0.5 rounded bg-accent/15 px-1 font-mono text-2xs text-accent hover:bg-accent/30">
        {source.n}
      </button>,
    );
    last = m.index! + m[0].length;
  }
  parts.push(text.slice(last));
  return <div className="selectable text-sm leading-relaxed whitespace-pre-wrap">{parts}</div>;
}

function AskTab({ projectId, ws, providers }: { projectId: string; ws: Workspace; providers: Provider[] }) {
  const [question, setQuestion] = useState("");
  // Starts on the Model Lab default when that model runs on this machine.
  const choice = useModelChoice(providers, "local");
  const providerId = choice.providerId;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [sources, setSources] = useState<Source[]>();
  const [answer, setAnswer] = useState<Answer>();
  const focus = ws.askFocus;
  const open = (s: Source) => ws.peek({ projectId, path: s.path, line: s.start_line });

  const ask = async () => {
    const q = question.trim() || (focus ? "Explain what this file does and how it relates to the files it imports and the files that use it." : "");
    if (!q) return;
    setBusy(true);
    setError(undefined);
    setAnswer(undefined);
    try {
      if (providerId) {
        const a = await api.askCodebase(projectId, q, providerId, choice.model.trim() || null, focus, choice.allowRemote);
        setAnswer(a);
        setSources(a.sources);
      } else {
        setSources(await api.findContext(projectId, q, focus));
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex-1 overflow-auto">
      <div className="mx-auto max-w-3xl space-y-4 p-5">
        <form className="space-y-2" onSubmit={(e) => { e.preventDefault(); void ask(); }}>
          {focus && (
            <div className="flex items-center gap-2 text-xs text-dim">
              Focused on <span className="font-mono text-ink">{focus}</span>
              <button type="button" aria-label="Clear focus" onClick={() => ws.askAbout(null)} className="text-faint hover:text-ink"><X size={12} /></button>
            </div>
          )}
          <textarea
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            rows={2}
            aria-label="Question"
            placeholder={focus ? "Ask about this file, or leave blank for an explanation of how it fits in…" : "Ask how something works, e.g. “how does task approval work?”"}
            className="w-full resize-none rounded-md border border-line bg-bg p-2.5 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none"
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void ask(); }
            }}
          />
          <div className="flex flex-wrap items-center gap-2">
            <ModelChoiceControls choice={choice} providers={providers} noneLabel="Find sources only (no AI)" idPrefix="ask" disabled={busy} />
            <Button type="submit" variant="primary" busy={busy} disabled={(!question.trim() && !focus) || !!choice.problem} title={choice.problem}>
              {providerId ? <><Sparkles size={13} /> Ask</> : <><Search size={13} /> Find sources</>}
            </Button>
            {providers.length === 0 && <span className="text-xs text-faint">Connect a model in Model Lab to get written answers. Source lookup works without one.</span>}
          </div>
          <ModelChoiceNotice choice={choice} what="Your question and the matching source excerpts from this project" />
        </form>
        <ErrorBanner message={error} />
        {answer && (
          <Card className="p-4" >
            <Cited text={answer.answer} sources={answer.sources} onOpen={open} />
            <div className="mt-3 text-2xs text-faint">
              {answer.model} · {(answer.duration_ms / 1000).toFixed(1)}s
              {answer.input_tokens !== null && ` · ${formatNumber(answer.input_tokens ?? 0)} in / ${formatNumber(answer.output_tokens ?? 0)} out tokens`}
            </div>
          </Card>
        )}
        {sources && (
          <div className="space-y-2" data-testid="sources">
            <div className="text-xs font-semibold text-dim">{sources.length ? `Sources (${sources.length})` : "No source files matched. Try naming a file, function or concept from the code."}</div>
            {sources.map((s) => (
              <Card key={s.n} className="overflow-hidden">
                <button onClick={() => open(s)} className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-raised">
                  <span className="rounded bg-accent/15 px-1.5 font-mono text-2xs text-accent">{s.n}</span>
                  <span className="truncate font-mono text-xs text-lav">{s.path}:{s.start_line}-{s.end_line}</span>
                  <span className="ml-auto shrink-0 truncate text-2xs text-faint">{s.reason}</span>
                </button>
                <pre className="selectable max-h-36 overflow-auto border-t border-line bg-bg px-3 py-2 font-mono text-2xs leading-4 text-dim">{s.text}</pre>
              </Card>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Docs

function DocsTab({ projectId, name, providers }: { projectId: string; name: string; providers: Provider[] }) {
  const choice = useModelChoice(providers, "none");
  const [doc, setDoc] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [note, setNote] = useState<string>();

  const generate = async (withProvider: string) => {
    setBusy(true);
    setError(undefined);
    setNote(undefined);
    try {
      setDoc(await api.architectureDoc(projectId, withProvider || null, withProvider ? choice.model.trim() || null : null, choice.allowRemote));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => { void generate(""); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [projectId]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2">
        <ModelChoiceControls choice={choice} providers={providers} noneLabel="Facts only (no AI narrative)" idPrefix="docs" disabled={busy} />
        <Button busy={busy} disabled={!!choice.problem} title={choice.problem} onClick={() => generate(choice.providerId)}><RefreshCw size={13} /> Generate</Button>
        <span className="text-xs text-faint">{note}</span>
        <div className="ml-auto flex gap-2">
          <Button disabled={!doc} onClick={async () => { await navigator.clipboard.writeText(doc!); setNote("Copied to clipboard."); }}><Copy size={13} /> Copy</Button>
          <Button
            variant="primary"
            disabled={!doc}
            onClick={async () => {
              try {
                const saved = await api.exportText(`${name}-ARCHITECTURE.md`, doc!);
                if (saved) setNote(`Saved to ${saved}`);
              } catch (e) {
                setError(errorText(e));
              }
            }}
          >
            <Download size={13} /> Save as…
          </Button>
          <Button
            title="A self-contained HTML report with the module graph, hotspots, dependencies and findings. It contains no source code."
            onClick={async () => {
              try {
                const saved = await api.saveProjectReport(projectId, true);
                if (saved) setNote(`Report saved to ${saved}`);
              } catch (e) {
                setError(errorText(e));
              }
            }}
          >
            <FileText size={13} /> Report (HTML)…
          </Button>
        </div>
      </div>
      <div className="empty:hidden space-y-2 p-3">
        <ErrorBanner message={error} />
        {choice.providerId && <ModelChoiceNotice choice={choice} what="The architecture facts of this project (module names, file counts, dependencies)" />}
      </div>
      <div className="min-h-0 flex-1" data-testid="arch-doc">
        {doc === undefined ? <Spinner label="Generating…" /> : (
          <Suspense fallback={<Spinner />}><CodeViewer path={`${projectId}/ARCHITECTURE.md`} value={doc} /></Suspense>
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Screen

export default function IntelligenceView({ ws }: { ws: Workspace }) {
  const project = ws.project;
  const id = project?.id ?? null;
  const [tab, setTab] = useState<Tab>("map");
  const [indexing, setIndexing] = useState<string | null>(null);
  const [error, setError] = useState<string>();
  const [version, setVersion] = useState(0);
  const status = useAsync(() => (id ? api.indexStatus(id) : Promise.resolve(undefined)), [id, version]);
  const providers = useAsync(api.listProviders, []);

  const rebuild = useMemo(() => async (projectId: string) => {
    setIndexing("Indexing…");
    setError(undefined);
    try {
      await api.rebuildIndex(projectId);
      setVersion((v) => v + 1);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setIndexing(null);
    }
  }, []);

  useEffect(() => {
    const sub = listen<{ project_id: string; done: number; total: number }>(INDEX_PROGRESS, (e) => {
      if (e.payload.project_id === id) setIndexing(`Indexing ${e.payload.done} of ${e.payload.total} files…`);
    });
    return () => void sub.then((off) => off());
  }, [id]);

  // First visit to a project that has never been indexed: build the index automatically.
  useEffect(() => {
    if (id && project?.exists && status.data && !status.data.indexed_at && !indexing) void rebuild(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, status.data?.indexed_at]);
  useEffect(() => { if (ws.askFocus) setTab("ask"); }, [ws.askFocus]);

  if (!project || !id)
    return (
      <>
        <ViewHeader title="Intelligence" />
        <Empty icon={<Network size={28} />} title="Import a project to explore its structure" />
      </>
    );

  const s = status.data;
  const ready = !!s?.indexed_at;
  return (
    <>
      <ViewHeader
        title="Intelligence"
        context={<span>{indexing ?? (ready ? `${project.name} · ${formatNumber(s.files)} files · ${formatNumber(s.symbols)} symbols · ${formatNumber(s.imports)} imports · indexed ${timeAgo(s.indexed_at)}` : `${project.name} · not indexed yet`)}</span>}
        actions={<Button size="sm" busy={!!indexing} disabled={!project.exists} onClick={() => rebuild(id)}><RefreshCw size={12} /> Rebuild index</Button>}
      />
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[{ id: "map", label: "Map" }, { id: "symbols", label: "Symbols" }, { id: "ask", label: "Ask" }, { id: "docs", label: "Docs" }]}
      />
      <div className="empty:hidden p-3"><ErrorBanner message={error ?? status.error} /></div>
      {!project.exists ? (
        <Empty title="Folder not found">This project's folder was moved or deleted.</Empty>
      ) : !ready ? (
        <Spinner label={indexing ?? "Preparing…"} />
      ) : (
        <div key={`${id}:${version}`} className="flex min-h-0 flex-1 flex-col">
          {tab === "map" && <MapTab projectId={id} ws={ws} />}
          {tab === "symbols" && <SymbolsTab projectId={id} />}
          {tab === "ask" && <AskTab projectId={id} ws={ws} providers={providers.data ?? []} />}
          {tab === "docs" && <DocsTab projectId={id} name={project.name} providers={providers.data ?? []} />}
        </div>
      )}
    </>
  );
}
