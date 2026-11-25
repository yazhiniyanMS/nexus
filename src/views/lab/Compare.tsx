// Model Comparison: specifications as reported, measurements from one saved benchmark, and
// the same prompt run against each model. No scores or rankings are computed.
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { Copy, FileDown, GitCompareArrows, Play, Square } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Badge, Button, Card, Empty, ErrorBanner, Select } from "../../components/ui";
import { api, LAB_INFER, type BenchSuite, type BenchTargetSummary, type InferEvent, type Metrics, type ModelRecord, type Stat } from "../../lib/api";
import { describeParams, fmtBytes, fmtInt, fmtMs, fmtParams, fmtStat, fmtTps, hostOf, newRequestId, normalizeParams, paramsFor, UNKNOWN } from "../../lib/lab";
import { markdownTable } from "../../lib/share";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { hardwareLine } from "./Benchmarks";
import { MetricsLine, type LabCtx, type LabTarget } from "./shared";

const MAX_MODELS = 4;
const localDate = (iso: string) => {
  const d = new Date(iso);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
const WHAT = "It contains model names, this machine, settings and measurements — no prompts or server addresses.";

interface SideRun { status: "waiting" | "running" | "done" | "error" | "stopped"; text: string; thinking: string; metrics?: Metrics; error?: string }

function Row({ label, title, cells, best }: { label: string; title?: string; cells: ReactNode[]; best?: number | null }) {
  return (
    <tr className="border-t border-line/60 align-top">
      <th className="w-40 py-1.5 pr-3 text-left text-xs font-normal text-faint" title={title}>{label}</th>
      {cells.map((c, i) => (
        <td key={i} className={clsx("selectable py-1.5 pr-4 text-sm break-words", c === UNKNOWN || c === null ? "text-faint" : "text-ink")}>
          {c ?? UNKNOWN}
          {best === i && <span className="ml-1.5 text-2xs text-accent" title="Clearly the best measured value in this row">best</span>}
        </td>
      ))}
    </tr>
  );
}

/**
 * Index of the model that is clearly best in a row: its whole range of runs must be better than
 * every other model's range. Overlapping ranges are run-to-run variation, not a difference.
 */
function bestOf(stats: (Stat | null | undefined)[], lowerIsBetter: boolean): number | null {
  const known = stats.map((s, i) => ({ s, i })).filter((x): x is { s: Stat; i: number } => !!x.s && x.s.n > 0);
  if (known.length < 2) return null;
  const sorted = [...known].sort((a, b) => (lowerIsBetter ? a.s.median - b.s.median : b.s.median - a.s.median));
  const best = sorted[0];
  const clear = sorted.slice(1).every((other) => (lowerIsBetter ? best.s.max < other.s.min : best.s.min > other.s.max));
  return clear ? best.i : null;
}

/** The same for single reported values such as memory. */
function lowestOf(values: (number | null | undefined)[]): number | null {
  const known = values.map((v, i) => ({ v, i })).filter((x): x is { v: number; i: number } => typeof x.v === "number" && x.v > 0);
  if (known.length < 2) return null;
  const sorted = [...known].sort((a, b) => a.v - b.v);
  return sorted[0].v === sorted[1].v ? null : sorted[0].i;
}

export default function CompareTab({ ctx, target }: { ctx: LabCtx; target: LabTarget | undefined }) {
  const models = useMemo(() => (ctx.library?.models ?? []).filter((m) => m.status !== "missing" && m.status !== "disabled"), [ctx.library]);
  const [selected, setSelected] = useState<string[]>([]);
  const suites = useAsync(api.labBenchList, []);
  const [suiteId, setSuiteId] = useState("");
  const [suite, setSuite] = useState<BenchSuite>();
  const [suiteError, setSuiteError] = useState<string>();
  const [prompt, setPrompt] = useState("");
  const [runs, setRuns] = useState<Record<string, SideRun>>({});
  const [active, setActive] = useState<{ id: string; key: string } | null>(null);
  const [allowRemote, setAllowRemote] = useState(false);
  const [error, setError] = useState<string>();
  const [shared, setShared] = useState<string>();
  const activeRef = useRef(active);
  const queue = useRef<string[]>([]);
  const promptRef = useRef("");
  const picked = useRef(false);
  activeRef.current = active;

  const chosen = selected.map((k) => models.find((m) => m.key === k)).filter((m): m is ModelRecord => !!m);
  const completed = (suites.data ?? []).filter((s) => s.status === "completed" || s.status === "cancelled");

  // Start with a sensible pair: favourites first, then whatever can be run.
  useEffect(() => {
    if (picked.current || models.length === 0 || target?.suiteId) return;
    picked.current = true;
    setSelected(models.filter((m) => m.provider_id).slice(0, 2).map((m) => m.key));
  }, [models, target]);

  // Arriving from a benchmark: use its results and select the models it tested.
  useEffect(() => {
    if (target?.suiteId) {
      picked.current = true;
      setSuiteId(target.suiteId);
    }
  }, [target]);

  useEffect(() => {
    void suites.reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  useEffect(() => {
    setSuite(undefined);
    setSuiteError(undefined);
    if (!suiteId) return;
    let live = true;
    api.labBenchGet(suiteId).then((s) => {
      if (!live) return;
      setSuite(s);
      if (target?.suiteId === suiteId) {
        const keys = s.config.targets.map((t) => models.find((m) => m.provider_id === t.provider_id && m.model_id === t.model)?.key).filter((k): k is string => !!k);
        if (keys.length) setSelected(keys.slice(0, MAX_MODELS));
      }
    }).catch((e) => live && setSuiteError(errorText(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [suiteId]);

  // Without an explicit choice, use the newest benchmark that covers the selection best.
  useEffect(() => {
    if (suiteId || completed.length === 0 || chosen.length === 0) return;
    const covers = (s: { models: string[] }) => chosen.filter((m) => s.models.includes(m.model_id)).length;
    const best = [...completed].sort((a, b) => covers(b) - covers(a))[0];
    if (best && covers(best) > 0) setSuiteId(best.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [completed.length, selected.join("|")]);

  const startNext = async () => {
    const key = queue.current.shift();
    if (!key) return setActive(null);
    const model = models.find((m) => m.key === key);
    const provider = ctx.providers.find((p) => p.id === model?.provider_id);
    if (!model || !provider) return void startNext();
    const id = newRequestId();
    setActive({ id, key });
    setRuns((r) => ({ ...r, [key]: { status: "running", text: "", thinking: "" } }));
    const kind = ctx.kinds.find((k) => k.kind === provider.kind);
    try {
      await api.labInferStart({
        id, provider_id: provider.id, model: model.model_id, system: null,
        messages: [{ role: "user", content: promptRef.current }],
        params: paramsFor(normalizeParams(ctx.settings?.params), kind?.params ?? []),
        allow_remote: allowRemote,
      });
    } catch (e) {
      setRuns((r) => ({ ...r, [key]: { status: "error", text: "", thinking: "", error: errorText(e) } }));
      void startNext();
    }
  };
  const nextRef = useRef(startNext);
  nextRef.current = startNext;

  useEffect(() => {
    const sub = listen<InferEvent>(LAB_INFER, (e) => {
      const current = activeRef.current;
      const ev = e.payload;
      if (!current || ev.id !== current.id) return;
      const key = current.key;
      if (ev.kind === "delta") {
        setRuns((r) => ({ ...r, [key]: { ...r[key], text: (r[key]?.text ?? "") + (ev.text ?? ""), thinking: (r[key]?.thinking ?? "") + (ev.thinking ?? ""), status: "running" } }));
      } else if (ev.kind === "done" && ev.outcome) {
        const o = ev.outcome;
        setRuns((r) => ({ ...r, [key]: { status: o.cancelled ? "stopped" : "done", text: o.text, thinking: o.thinking, metrics: o.metrics } }));
        void nextRef.current();
      } else if (ev.kind === "error") {
        setRuns((r) => ({ ...r, [key]: { ...r[key], status: "error", error: ev.message ?? "The request failed" } }));
        void nextRef.current();
      }
    });
    return () => void sub.then((off) => off());
  }, []);

  const runnable = chosen.filter((m) => m.provider_id && ctx.providers.some((p) => p.id === m.provider_id && p.enabled));
  const remoteHosts = [...new Set(runnable.map((m) => ctx.providers.find((p) => p.id === m.provider_id)).filter((p) => p && !p.local).map((p) => hostOf(p!.base_url)))];

  const runAll = () => {
    setError(undefined);
    if (!prompt.trim()) return;
    promptRef.current = prompt.trim();
    queue.current = runnable.map((m) => m.key);
    setRuns(Object.fromEntries(queue.current.map((k) => [k, { status: "waiting", text: "", thinking: "" } as SideRun])));
    void startNext();
  };
  const stop = () => {
    queue.current = [];
    if (active) void api.labInferCancel(active.id);
  };

  const toggle = (key: string) => setSelected((s) => (s.includes(key) ? s.filter((k) => k !== key) : s.length >= MAX_MODELS ? s : [...s, key]));

  if (models.length === 0) {
    return (
      <Empty icon={<GitCompareArrows size={28} />} title="Nothing to compare yet">
        <p className="mb-3">Comparison works on the models in your library. Connect a runtime or add a model first.</p>
        <Button variant="primary" onClick={() => ctx.go("library")}>Open the library</Button>
      </Empty>
    );
  }

  const summaryOf = (m: ModelRecord): BenchTargetSummary | undefined => {
    const i = suite?.config.targets.findIndex((t) => t.provider_id === m.provider_id && t.model === m.model_id) ?? -1;
    return i >= 0 ? suite?.summary[i] : undefined;
  };
  const summaries = chosen.map(summaryOf);
  const anyMeasured = summaries.some((s) => s && s.runs > 0);
  const notIn = "not in this benchmark";
  const stat = (pick: (s: BenchTargetSummary) => ReactNode) => summaries.map((s) => (s ? (s.runs > 0 ? pick(s) : "no successful run") : suite ? notIn : UNKNOWN));
  // What "Copy/Save as Markdown" shares: model names, the machine, settings and measurements.
  // Never the benchmark's name, its prompts or the address of a remote server.
  const shareText = () => {
    if (!suite) return "";
    const s = (pick: (x: BenchTargetSummary) => string) => summaries.map((x) => (x ? (x.runs > 0 ? pick(x) : "no successful run") : notIn));
    return markdownTable({
      title: "Local model benchmark",
      context: [
        `Machine: ${hardwareLine(suite.hardware)}`,
        `Settings: ${describeParams(suite.config.params)} · ${suite.config.prompts.length} prompt${suite.config.prompts.length === 1 ? "" : "s"} × ${suite.config.repeats} run${suite.config.repeats === 1 ? "" : "s"}${suite.config.warmup ? " · warmed up" : ""}`,
        `Measured ${localDate(suite.created_at)}. Median (range, runs) where several runs were made.`,
      ],
      columns: chosen.map((m) => `${m.model_id} (${m.local ? "local" : "remote"})`),
      rows: [
        { label: "First token", cells: s((x) => fmtStat(x.ttft_ms, fmtMs)) },
        { label: "Generation speed", cells: s((x) => `${fmtStat(x.tokens_per_second, fmtTps)}${x.tps_source && x.tps_source !== "runtime" ? ` (${x.tps_source})` : ""}`) },
        { label: "Total time", cells: s((x) => fmtStat(x.total_ms, fmtMs)) },
        { label: "Model load", cells: s((x) => fmtMs(x.load_ms)) },
        { label: "Memory when loaded", cells: s((x) => (x.memory_bytes ? fmtBytes(x.memory_bytes) : UNKNOWN)) },
        { label: "Output tokens", cells: s((x) => fmtStat(x.output_tokens, (v) => fmtInt(v))) },
        { label: "Measured runs", cells: s((x) => `${x.runs}${x.errors ? ` (${x.errors} failed)` : ""}`) },
      ],
      note: "Speed only, on this one machine; output quality was not measured.",
    });
  };
  const untested = suite ? suite.config.targets.filter((t) => !chosen.some((m) => m.provider_id === t.provider_id && m.model_id === t.model)) : [];

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-64 shrink-0 flex-col border-r border-line">
        <div className="flex h-11 shrink-0 items-center border-b border-line px-3 text-xs font-semibold">Models to compare ({selected.length} of {MAX_MODELS})</div>
        <div className="flex-1 overflow-auto p-2">
          {models.map((m) => (
            <label key={m.key} data-compare-model={m.model_id} className={clsx("mb-0.5 flex cursor-default items-start gap-2 rounded-md px-2 py-1.5", selected.includes(m.key) ? "bg-raised" : "hover:bg-raised/60", !selected.includes(m.key) && selected.length >= MAX_MODELS && "opacity-50")}>
              <input type="checkbox" className="mt-0.5 accent-accent" checked={selected.includes(m.key)} disabled={!!active || (!selected.includes(m.key) && selected.length >= MAX_MODELS)} onChange={() => toggle(m.key)} />
              <span className="min-w-0">
                <span className="block truncate font-mono text-xs">{m.model_id}</span>
                <span className="block truncate text-2xs text-faint">{m.provider_name ?? "Model folder"} · {m.local ? "local" : "remote"}</span>
              </span>
            </label>
          ))}
        </div>
      </aside>

      <div className="min-w-0 flex-1 overflow-auto">
        {chosen.length === 0 ? (
          <Empty icon={<GitCompareArrows size={28} />} title="Choose models to compare">Tick up to {MAX_MODELS} models on the left.</Empty>
        ) : (
          <div className="mx-auto w-full max-w-6xl space-y-5 p-5">
            <Card className="p-4">
              <h2 className="text-sm font-semibold">Specifications</h2>
              <p className="mt-0.5 text-xs text-dim">As reported by each runtime or read from the model file. A dash means it was not reported.</p>
              <table data-testid="compare-specs" className="mt-2 w-full table-fixed">
                <thead>
                  <tr>
                    <th className="w-40" />
                    {chosen.map((m) => <th key={m.key} className="pb-1 pr-4 text-left font-mono text-xs font-semibold break-all">{m.model_id}</th>)}
                  </tr>
                </thead>
                <tbody>
                  <Row label="Runs on" cells={chosen.map((m) => m.compatibility.join("; "))} />
                  <Row label="Where" cells={chosen.map((m) => (m.kind === "file" ? "file on this machine" : m.local ? `${m.provider_name} · local` : `${m.provider_name} · remote`))} />
                  <Row label="Parameters" cells={chosen.map((m) => fmtParams(m.meta.parameter_count, m.meta.parameter_label))} />
                  <Row label="Quantization" cells={chosen.map((m) => m.meta.quantization ?? UNKNOWN)} />
                  <Row label="Context length" cells={chosen.map((m) => (m.meta.context_length ? `${fmtInt(m.meta.context_length)} tokens` : UNKNOWN))} />
                  <Row label="Max output" cells={chosen.map((m) => (m.meta.max_output_tokens ? `${fmtInt(m.meta.max_output_tokens)} tokens` : UNKNOWN))} />
                  <Row label="Size on disk" cells={chosen.map((m) => (m.meta.size_bytes ? fmtBytes(m.meta.size_bytes) : UNKNOWN))} />
                  <Row label="Format" cells={chosen.map((m) => m.meta.format ?? UNKNOWN)} />
                  <Row label="Capabilities" cells={chosen.map((m) => (m.meta.capabilities.length ? m.meta.capabilities.join(", ") : UNKNOWN))} />
                  <Row label="Licence" cells={chosen.map((m) => m.meta.license ?? UNKNOWN)} />
                  <Row label="Details from" cells={chosen.map((m) => <span className="text-xs text-dim">{m.meta_source || UNKNOWN}</span>)} />
                </tbody>
              </table>
            </Card>

            <Card className="p-4">
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="text-sm font-semibold">Measured performance</h2>
                <Select aria-label="Benchmark to use" className="ml-auto w-64!" value={suiteId} onChange={(e) => setSuiteId(e.target.value)}>
                  <option value="">{completed.length ? "Choose a saved benchmark…" : "No saved benchmark yet"}</option>
                  {completed.map((s) => <option key={s.id} value={s.id}>{s.name} · {timeAgo(s.created_at)}</option>)}
                </Select>
                {suite && (
                  <>
                    <Button size="sm" title="Model names, this machine, settings and measurements as a Markdown table. Not the benchmark's name, prompts or server addresses." onClick={async () => {
                      try {
                        await navigator.clipboard.writeText(shareText());
                        setShared(`Copied as Markdown. ${WHAT}`);
                      } catch (e) {
                        setShared(`Could not copy: ${errorText(e)}`);
                      }
                    }}><Copy size={12} /> Copy table</Button>
                    <Button size="sm" onClick={async () => {
                      try {
                        const path = await api.exportText("nexus-benchmark.md", shareText());
                        if (path) setShared(`Saved. ${WHAT}`);
                      } catch (e) {
                        setShared(`Could not save: ${errorText(e)}`);
                      }
                    }}><FileDown size={12} /> Save…</Button>
                  </>
                )}
                <Button size="sm" onClick={() => ctx.go("benchmarks", runnable[0] ? { providerId: runnable[0].provider_id ?? undefined, model: runnable[0].model_id } : undefined)}>Run a benchmark</Button>
              </div>
              <ErrorBanner message={suiteError ?? suites.error} />
              {!suite ? (
                <p data-testid="compare-no-suite" className="mt-2 text-xs leading-relaxed text-dim">
                  {completed.length === 0
                    ? "No benchmark has been run yet. Speed is only shown from real measurements, so run a benchmark with these models to fill this in."
                    : "Choose a benchmark. Figures from one benchmark share the same prompts, settings and machine, which is what makes them comparable."}
                </p>
              ) : (
                <>
                  <div data-testid="compare-conditions" className="selectable mt-2 space-y-0.5 rounded-md border border-line/70 bg-bg p-2 text-xs text-dim">
                    <div><span className="text-faint">From:</span> {suite.name}, run {new Date(suite.created_at).toLocaleString()}</div>
                    <div><span className="text-faint">Machine:</span> {hardwareLine(suite.hardware)}</div>
                    <div><span className="text-faint">Settings:</span> {describeParams(suite.config.params)} · {suite.config.prompts.length} prompt{suite.config.prompts.length === 1 ? "" : "s"} × {suite.config.repeats} run{suite.config.repeats === 1 ? "" : "s"}{suite.config.warmup ? " · warmed up" : ""}</div>
                  </div>
                  <table data-testid="compare-perf" className="mt-2 w-full table-fixed">
                    <thead>
                      <tr>
                        <th className="w-40" />
                        {chosen.map((m) => <th key={m.key} className="pb-1 pr-4 text-left font-mono text-xs font-semibold break-all">{m.model_id}</th>)}
                      </tr>
                    </thead>
                    <tbody>
                      <Row label="First token" title="Median, range and number of runs. Measured by NEXUS." cells={stat((s) => fmtStat(s.ttft_ms, fmtMs))} best={bestOf(summaries.map((s) => s?.ttft_ms), true)} />
                      <Row label="Generation speed" title="Median tokens per second" cells={stat((s) => <>{fmtStat(s.tokens_per_second, fmtTps)}{s.tps_source && s.tps_source !== "runtime" ? <span className="text-2xs text-warn"> {s.tps_source}</span> : null}</>)} best={bestOf(summaries.map((s) => s?.tokens_per_second), false)} />
                      <Row label="Total time" cells={stat((s) => fmtStat(s.total_ms, fmtMs))} best={bestOf(summaries.map((s) => s?.total_ms), true)} />
                      <Row label="Prompt processing" cells={stat((s) => fmtStat(s.prompt_eval_ms, fmtMs))} />
                      <Row label="Model load" title="Load time the runtime reported for the first request" cells={stat((s) => fmtMs(s.load_ms))} />
                      <Row label="Memory when loaded" cells={stat((s) => (s.memory_bytes ? `${fmtBytes(s.memory_bytes)}${s.memory_vram_bytes !== null && s.memory_bytes ? ` (${Math.round((s.memory_vram_bytes / s.memory_bytes) * 100)}% on GPU)` : ""}` : UNKNOWN))} best={lowestOf(summaries.map((s) => s?.memory_bytes))} />
                      <Row label="Output tokens" title="Median output length; shorter replies finish sooner" cells={stat((s) => fmtStat(s.output_tokens, (v) => fmtInt(v)))} />
                      <Row label="Measured runs" cells={stat((s) => `${s.runs}${s.errors ? ` (${s.errors} failed)` : ""}`)} />
                    </tbody>
                  </table>
                  {shared && <p data-testid="compare-shared" className="mt-2 text-2xs text-faint">{shared}</p>}
                  <p className="mt-2 text-2xs leading-relaxed text-faint">
                    “Best” appears only where one model's whole range of runs beats every other model's range; overlapping ranges are run-to-run variation, so no model is marked. It is not a quality rating: output quality is not measured here.
                    {!anyMeasured && " None of the selected models has a successful run in this benchmark."}
                    {untested.length > 0 && ` This benchmark also tested ${untested.map((t) => t.model).join(", ")}.`}
                  </p>
                </>
              )}
            </Card>

            <Card className="p-4">
              <h2 className="text-sm font-semibold">Same prompt, side by side</h2>
              <p className="mt-0.5 text-xs text-dim">Runs one prompt against each selected model in turn, with your default generation settings ({describeParams(normalizeParams(ctx.settings?.params))}).</p>
              <div className="mt-2 flex items-end gap-2">
                <textarea aria-label="Comparison prompt" rows={2} value={prompt} onChange={(e) => setPrompt(e.target.value)} disabled={!!active} placeholder="A prompt to send to every selected model" className="flex-1 resize-y rounded-md border border-line bg-bg p-2 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none" />
                {active
                  ? <Button variant="danger" onClick={stop}><Square size={12} /> Stop</Button>
                  : <Button variant="primary" disabled={!prompt.trim() || runnable.length === 0 || (remoteHosts.length > 0 && !allowRemote)} onClick={runAll}><Play size={12} /> Run on {runnable.length} model{runnable.length === 1 ? "" : "s"}</Button>}
              </div>
              {remoteHosts.length > 0 && (
                <label data-testid="remote-notice" className="mt-2 flex items-start gap-2 rounded-md border border-warn/30 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
                  <input type="checkbox" className="mt-0.5 accent-warn" checked={allowRemote} onChange={(e) => setAllowRemote(e.target.checked)} />
                  <span><span className="font-semibold">This leaves your machine.</span> The prompt will be sent to {remoteHosts.join(", ")}. Tick to allow it.</span>
                </label>
              )}
              {chosen.length > runnable.length && <p className="mt-2 text-2xs text-faint">Models that no connected runtime is serving (model files, disabled connections) are skipped.</p>}
              <ErrorBanner message={error} />
              {Object.keys(runs).length > 0 && (
                <div data-testid="compare-outputs" className="mt-3 grid gap-3" style={{ gridTemplateColumns: `repeat(${Math.min(runnable.length, MAX_MODELS)}, minmax(0, 1fr))` }}>
                  {runnable.map((m) => {
                    const r = runs[m.key];
                    return (
                      <div key={m.key} data-output={m.model_id} data-status={r?.status} className="min-w-0 rounded-md border border-line p-3">
                        <div className="flex items-center gap-2">
                          <span className="min-w-0 flex-1 truncate font-mono text-xs font-semibold">{m.model_id}</span>
                          {r && <Badge tone={r.status === "done" ? "green" : r.status === "error" ? "red" : r.status === "running" ? "blue" : "neutral"}>{r.status}</Badge>}
                        </div>
                        {r?.thinking && <div className="mt-1 text-2xs text-faint">thinking: {r.thinking.length.toLocaleString()} characters</div>}
                        <div className="selectable mt-2 max-h-80 overflow-auto text-sm leading-relaxed break-words whitespace-pre-wrap">{r?.text || (r?.status === "waiting" ? <span className="text-faint">Waiting for its turn…</span> : r?.status === "running" ? <span className="text-faint">Waiting for the first token…</span> : null)}</div>
                        {r?.error && <div className="selectable mt-2 text-xs text-danger">{r.error}</div>}
                        {r?.metrics && <MetricsLine metrics={r.metrics} className="mt-2 border-t border-line/60 pt-2" />}
                      </div>
                    );
                  })}
                </div>
              )}
            </Card>
          </div>
        )}
      </div>
    </div>
  );
}
