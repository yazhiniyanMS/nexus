// Model Library: every model the connected runtimes and the model folder report.
import clsx from "clsx";
import { ArrowDownAZ, ArrowUpAZ, Download as DownloadIcon, FlaskConical, FolderOpen, Gauge, Play, RefreshCw, Star, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge, Button, Card, Empty, ErrorBanner, Input, Select, Spinner } from "../../components/ui";
import { api, type ModelRecord } from "../../lib/api";
import { filterModels, fmtBytes, fmtInt, fmtParams, sortModels, UNKNOWN, type SortKey } from "../../lib/lab";
import { errorText, timeAgo } from "../../lib/util";
import { AddModel } from "./AddModel";
import { ConfirmButton, CopyButton, type LabCtx } from "./shared";
import { StoragePanel } from "./Storage";

const STATUS_LABEL: Record<ModelRecord["status"], string> = {
  installed: "installed",
  remote: "remote",
  file: "file on disk",
  disabled: "connection disabled",
  missing: "no longer listed",
};
const STATUS_TONE: Record<ModelRecord["status"], "green" | "amber" | "blue" | "red" | "neutral"> = { installed: "green", remote: "amber", file: "blue", disabled: "neutral", missing: "red" };

const sourceName = (m: ModelRecord) => m.provider_name ?? "Model folder";

function Fact({ label, value, mono, title }: { label: string; value: string | null | undefined; mono?: boolean; title?: string }) {
  const shown = value && value.trim() ? value : UNKNOWN;
  return (
    <div className="flex gap-3 py-1" title={shown === UNKNOWN ? "Not reported for this model" : title}>
      <dt className="w-32 shrink-0 text-xs text-faint">{label}</dt>
      <dd className={clsx("selectable min-w-0 flex-1 break-words text-sm", shown === UNKNOWN ? "text-faint" : "text-ink", mono && "font-mono text-xs")}>{shown}</dd>
    </div>
  );
}

function Detail({ ctx, model, onChanged }: { ctx: LabCtx; model: ModelRecord; onChanged: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [showLicense, setShowLicense] = useState(false);
  const m = model.meta;
  const isDefault = ctx.settings?.default_provider_id === model.provider_id && ctx.settings?.default_model === model.model_id;
  const canRun = model.provider_id !== null && model.status !== "missing" && model.status !== "disabled";

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await fn();
      onChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const removeLabel = model.kind === "file" ? "Delete file" : `Remove from ${model.provider_name}`;
  const removeConfirm = `${removeLabel}${m.size_bytes ? ` (${fmtBytes(m.size_bytes)})` : ""}: cannot be undone`;

  return (
    <div data-testid="model-detail" className="space-y-4 p-4">
      <div>
        <div className="flex items-start gap-2">
          <h2 className="selectable min-w-0 flex-1 break-all font-mono text-sm font-semibold">{model.model_id}</h2>
          <button
            title={model.favorite ? "Remove from favourites" : "Mark as favourite"}
            aria-pressed={model.favorite}
            className={clsx("grid size-7 shrink-0 place-items-center rounded hover:bg-raised", model.favorite ? "text-warn" : "text-faint")}
            onClick={() => void run(() => api.labModelFavorite(model.key, !model.favorite))}
          >
            <Star size={14} fill={model.favorite ? "currentColor" : "none"} />
          </button>
        </div>
        {m.display_name && <div className="text-sm text-dim">{m.display_name}</div>}
        <div className="mt-2 flex flex-wrap gap-1.5">
          <Badge tone={STATUS_TONE[model.status]}>{STATUS_LABEL[model.status]}</Badge>
          <Badge tone={model.local ? "green" : "amber"}>{model.local ? "local" : "remote"}</Badge>
          {model.managed && <Badge>downloaded by NEXUS</Badge>}
          {isDefault && <Badge tone="blue">default model</Badge>}
        </div>
      </div>
      <ErrorBanner message={error} />

      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="primary" disabled={!canRun} title={canRun ? undefined : "No connected runtime is serving this model"} onClick={() => ctx.go("playground", { providerId: model.provider_id ?? undefined, model: model.model_id })}>
          <Play size={12} /> Open in Playground
        </Button>
        <Button size="sm" disabled={!canRun} onClick={() => ctx.go("benchmarks", { providerId: model.provider_id ?? undefined, model: model.model_id })}><Gauge size={12} /> Benchmark</Button>
        <Button size="sm" disabled={!canRun || isDefault || busy} title="Use this model by default in the Playground, project questions and new agent tasks" onClick={() => void run(async () => {
          await api.labSettingsSet(model.provider_id, model.model_id, ctx.settings?.params ?? { temperature: null, top_p: null, top_k: null, max_tokens: null, seed: null, repeat_penalty: null, think: null });
          await ctx.reloadSettings();
        })}>
          <FlaskConical size={12} /> {isDefault ? "Default model" : "Set as default"}
        </Button>
      </div>
      {model.kind === "file" && (
        <p className="text-xs leading-relaxed text-dim">
          This is a model file, not a running model. Start a llama.cpp server for it in Runtimes, or import it into Ollama, to use it.
        </p>
      )}

      <dl className="divide-y divide-line/60 border-y border-line/60">
        <Fact label="Source" value={sourceName(model)} />
        <Fact label="Family" value={m.family} />
        <Fact label="Architecture" value={m.architecture} />
        <Fact label="Parameters" value={fmtParams(m.parameter_count, m.parameter_label)} title={m.parameter_count ? `${fmtInt(m.parameter_count)} parameters` : undefined} />
        <Fact label="Quantization" value={m.quantization} />
        <Fact label="Context length" value={m.context_length ? `${fmtInt(m.context_length)} tokens` : null} />
        {m.max_output_tokens !== null && <Fact label="Max output" value={`${fmtInt(m.max_output_tokens)} tokens`} />}
        <Fact label="Capabilities" value={m.capabilities.join(", ")} />
        <Fact label="Format" value={m.format} />
        <Fact label="Size on disk" value={m.size_bytes ? fmtBytes(m.size_bytes) : null} title={m.size_bytes ? `${fmtInt(m.size_bytes)} bytes` : undefined} />
        <Fact label="Runs on" value={model.compatibility.join("; ")} />
        <Fact label="Licence" value={m.license} />
        {m.source_url && <Fact label="Homepage" value={m.source_url} mono />}
        {m.path && <Fact label="Path" value={m.path} mono />}
        {m.digest && <Fact label="Digest" value={m.digest} mono />}
        <Fact label="Updated" value={m.modified_at ? timeAgo(m.modified_at) : null} title={m.modified_at ?? undefined} />
        <Fact label="Last seen" value={timeAgo(model.last_seen)} title={model.last_seen} />
      </dl>

      <div className="space-y-1 text-xs leading-relaxed text-dim">
        <div><span className="text-faint">Where these details come from:</span> {model.meta_source || "unknown"}.</div>
        <div className="text-faint">A dash means the runtime or file did not state the value. NEXUS does not fill in specifications from elsewhere.</div>
        {m.notes.map((n) => <div key={n}>· {n}</div>)}
      </div>

      {m.runtime_defaults && (
        <div>
          <div className="mb-1 text-xs font-medium text-dim">Runtime's default settings for this model</div>
          <pre className="selectable overflow-auto rounded-md border border-line bg-bg p-2 font-mono text-xs leading-5 text-dim">{m.runtime_defaults}</pre>
        </div>
      )}
      {m.license_text && (
        <div>
          <button className="text-xs text-accent hover:underline" onClick={() => setShowLicense((v) => !v)}>{showLicense ? "Hide licence text" : "Show licence text"}</button>
          {showLicense && <pre className="selectable mt-1 max-h-64 overflow-auto rounded-md border border-line bg-bg p-2 font-mono text-2xs leading-5 whitespace-pre-wrap text-dim">{m.license_text}</pre>}
        </div>
      )}

      <div className="flex items-center gap-2 border-t border-line/60 pt-3">
        <CopyButton text={model.model_id} label="Copy identifier" />
        {model.removable ? (
          <ConfirmButton label={removeLabel} confirm={removeConfirm} busy={busy} icon={<Trash2 size={12} />} onConfirm={() => run(async () => {
            await api.labModelRemove(model.key);
            await ctx.refreshLibrary();
          })} />
        ) : (
          <span className="text-2xs text-faint" title="NEXUS only removes files it downloaded and models in a local Ollama">Not removable from NEXUS</span>
        )}
      </div>
    </div>
  );
}

export default function LibraryTab({ ctx }: { ctx: LabCtx }) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [status, setStatus] = useState("all");
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [sort, setSort] = useState<SortKey>("name");
  const [descending, setDescending] = useState(false);
  const [selected, setSelected] = useState<string>();
  const [panel, setPanel] = useState<"detail" | "add" | "storage">("detail");

  const models = ctx.library?.models ?? [];
  const sources = useMemo(() => [...new Set(models.map(sourceName))].sort(), [models]);
  const shown = useMemo(
    () => sortModels(filterModels(models, { query, source, status, favoritesOnly }), sort, descending),
    [models, query, source, status, favoritesOnly, sort, descending],
  );
  const current = models.find((m) => m.key === selected);
  // The same model served by two connections to one runtime is one copy on disk.
  const counted = new Set<string>();
  const totalBytes = models.reduce((sum, m) => {
    if (!(m.status === "installed" || m.status === "file") || m.meta.size_bytes === null) return sum;
    const identity = m.meta.digest ?? m.key;
    if (counted.has(identity)) return sum;
    counted.add(identity);
    return sum + m.meta.size_bytes;
  }, 0);
  const failedSources = (ctx.library?.sources ?? []).filter((s) => !s.ok && s.error !== "Disabled");
  // Models of a runtime that did not answer are the last known list, not a live one.
  const offline = new Set(failedSources.map((s) => s.provider_id));

  if (!ctx.library) return ctx.libraryError ? <div className="p-5"><ErrorBanner message={ctx.libraryError} /></div> : <Spinner label="Loading the model library…" />;

  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-4 py-2">
          <Input aria-label="Search models" className="w-56!" placeholder="Search models" value={query} onChange={(e) => setQuery(e.target.value)} />
          <Select aria-label="Source" className="w-40!" value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="all">All sources</option>
            {sources.map((s) => <option key={s} value={s}>{s}</option>)}
          </Select>
          <Select aria-label="Status" className="w-36!" value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="all">Any status</option>
            <option value="installed">Installed</option>
            <option value="file">Files on disk</option>
            <option value="remote">Remote</option>
            <option value="disabled">Connection disabled</option>
            <option value="missing">No longer listed</option>
          </Select>
          <button
            aria-pressed={favoritesOnly}
            title="Show favourites only"
            onClick={() => setFavoritesOnly((v) => !v)}
            className={clsx("grid size-8 place-items-center rounded-md border", favoritesOnly ? "border-warn/50 bg-warn/10 text-warn" : "border-line text-dim hover:text-ink")}
          >
            <Star size={13} fill={favoritesOnly ? "currentColor" : "none"} />
          </button>
          <Select aria-label="Sort by" className="w-36!" value={sort} onChange={(e) => setSort(e.target.value as SortKey)}>
            <option value="name">Sort: name</option>
            <option value="size">Sort: size</option>
            <option value="provider">Sort: source</option>
            <option value="status">Sort: status</option>
            <option value="params">Sort: parameters</option>
            <option value="context">Sort: context</option>
          </Select>
          <button title={descending ? "Descending" : "Ascending"} aria-label="Sort direction" onClick={() => setDescending((d) => !d)} className="grid size-8 place-items-center rounded-md border border-line text-dim hover:text-ink">
            {descending ? <ArrowUpAZ size={14} /> : <ArrowDownAZ size={14} />}
          </button>
          <div className="ml-auto flex items-center gap-2">
            <Button size="sm" busy={ctx.libraryBusy} onClick={() => void ctx.refreshLibrary()} title="Ask every enabled connection and the model folder what they have"><RefreshCw size={12} /> Refresh</Button>
            <Button size="sm" variant={panel === "storage" ? "primary" : "default"} onClick={() => setPanel(panel === "storage" ? "detail" : "storage")}><FolderOpen size={12} /> Storage</Button>
            <Button size="sm" variant={panel === "add" ? "primary" : "default"} onClick={() => setPanel(panel === "add" ? "detail" : "add")}><DownloadIcon size={12} /> Add model</Button>
          </div>
        </div>

        {(failedSources.length > 0 || ctx.libraryError) && (
          <div className="space-y-1 border-b border-line px-4 py-2">
            <ErrorBanner message={ctx.libraryError} />
            {failedSources.map((s) => (
              <div key={s.provider_id ?? s.name} data-testid="source-error" className="text-xs text-warn">
                <span className="font-medium">{s.name}</span> could not be reached, so its models below are the last known list. <span className="text-warn/70">{s.error}</span>
              </div>
            ))}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-auto">
          {models.length === 0 ? (
            <Empty icon={<FlaskConical size={28} />} title="No models yet">
              <p className="mb-3">
                The library lists what your connected runtimes serve and the model files in your model folder.
                {ctx.providers.length === 0 ? " No runtime is connected yet." : " Your connections did not report any models."}
              </p>
              <div className="flex justify-center gap-2">
                <Button variant="primary" onClick={() => ctx.go("runtimes")}>Find runtimes on this machine</Button>
                <Button onClick={() => setPanel("add")}>Add a model</Button>
              </div>
            </Empty>
          ) : shown.length === 0 ? (
            <Empty title="No model matches these filters">Clear the search or change the filters to see all {models.length} models.</Empty>
          ) : (
            <table data-testid="model-table" className="w-full text-sm whitespace-nowrap">
              <thead className="sticky top-0 z-10 bg-bg text-left text-xs text-faint">
                <tr className="border-b border-line">
                  <th className="w-8 py-2 pl-4" />
                  <th className="w-full py-2 font-medium">Model</th>
                  <th className="pr-3 font-medium">Source</th>
                  <th className="pr-3 font-medium">Parameters</th>
                  <th className="pr-3 font-medium">Quantization</th>
                  <th className="pr-3 text-right font-medium">Context</th>
                  <th className="pr-3 text-right font-medium">Size</th>
                  <th className="px-4 font-medium">Status</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line/60">
                {shown.map((m) => (
                  <tr
                    key={m.key}
                    data-model={m.model_id}
                    onClick={() => { setSelected(m.key); setPanel("detail"); }}
                    className={clsx("cursor-default", selected === m.key && panel === "detail" ? "bg-raised" : "hover:bg-raised/50")}
                  >
                    <td className="py-2 pl-4 text-warn">{m.favorite && <Star size={12} fill="currentColor" />}</td>
                    <td className="max-w-0 min-w-32 py-2 pr-3">
                      <div className="truncate font-mono text-xs text-ink">{m.model_id}</div>
                      {m.meta.display_name && <div className="truncate text-2xs text-faint">{m.meta.display_name}</div>}
                    </td>
                    <td className="pr-3 text-dim">{sourceName(m)}</td>
                    <td className="pr-3 text-dim tabular-nums">{fmtParams(m.meta.parameter_count, m.meta.parameter_label)}</td>
                    <td className="pr-3 text-dim">{m.meta.quantization ?? UNKNOWN}</td>
                    <td className="pr-3 text-right text-dim tabular-nums">{m.meta.context_length ? fmtInt(m.meta.context_length) : UNKNOWN}</td>
                    <td className="pr-3 text-right text-dim tabular-nums">{m.meta.size_bytes ? fmtBytes(m.meta.size_bytes) : UNKNOWN}</td>
                    <td className="px-4">
                      {m.provider_id && offline.has(m.provider_id)
                        ? <Badge tone="amber">runtime offline</Badge>
                        : <Badge tone={STATUS_TONE[m.status]}>{STATUS_LABEL[m.status]}</Badge>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div data-testid="library-footer" className="flex shrink-0 items-center gap-4 border-t border-line px-4 py-1.5 text-2xs text-faint">
          <span>{shown.length} of {models.length} models</span>
          <span>{fmtBytes(totalBytes)} on disk across installed models and files</span>
          <span className="ml-auto">Values shown as {UNKNOWN} were not reported.</span>
        </div>
      </div>

      <aside className="flex w-[27rem] shrink-0 flex-col overflow-auto border-l border-line max-[1280px]:w-80 max-[1080px]:w-64">
        {panel === "add" ? (
          <AddModel ctx={ctx} />
        ) : panel === "storage" ? (
          <StoragePanel ctx={ctx} />
        ) : current ? (
          <Detail key={current.key} ctx={ctx} model={current} onChanged={() => void ctx.refreshLibrary()} />
        ) : (
          <Card className="m-4 p-4 text-sm leading-relaxed text-dim">
            Select a model to see what its runtime reports about it: size, quantization, context length, capabilities and licence.
          </Card>
        )}
      </aside>
    </div>
  );
}
