// Benchmark Lab: run the same prompts with the same settings against chosen models, several
// times each, and keep the measurements with the conditions they were taken under.
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { ChevronDown, ChevronRight, Download as DownloadIcon, Gauge, GitCompareArrows, Play, Plus, Square, Trash2, X } from "lucide-react";
import { Fragment, useEffect, useMemo, useState } from "react";
import { Badge, Button, Card, Empty, ErrorBanner, Field, Input, Spinner } from "../../components/ui";
import { api, LAB_BENCH, type BenchPrompt, type BenchSuite, type BenchTarget, type BenchTargetInfo, type GenParams, type Hardware, type KindInfo } from "../../lib/api";
import { describeParams, emptyParams, fmtBytes, fmtInt, fmtMs, fmtStat, fmtTps, hostOf, percent, UNKNOWN } from "../../lib/lab";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { ConfirmButton, CopyButton, ModelPicker, ParamsEditor, Progress, usable, type LabCtx, type LabTarget } from "./shared";

const PRESETS: BenchPrompt[] = [
  { label: "Short answer", text: "Reply with the single word: ready." },
  { label: "Explanation", text: "Explain what a hash map is and when to use one, in about 150 words." },
  { label: "Code", text: "Write a Python function that merges two sorted lists into one sorted list. Include a docstring." },
];

const STATUS_TONE: Record<BenchSuite["status"], "green" | "amber" | "blue" | "red" | "neutral"> = {
  running: "blue", completed: "green", cancelled: "neutral", failed: "red", interrupted: "amber",
};

export function hardwareLine(hw: Hardware): string {
  return [
    `${hw.os}${hw.os_version ? ` ${hw.os_version}` : ""}`,
    hw.arch,
    hw.cpu ?? "CPU not reported",
    hw.cores !== null ? `${hw.cores} cores` : null,
    hw.memory_bytes !== null ? `${fmtBytes(hw.memory_bytes)} memory` : "memory not reported",
  ].filter(Boolean).join(" · ");
}

export function targetLine(t: BenchTargetInfo): string {
  return [
    `${t.provider_name} (${t.runtime}${t.runtime_version ? ` ${t.runtime_version}` : ""})`,
    t.local ? "local" : `remote · ${t.endpoint}`,
    t.parameters ? `${t.parameters} parameters` : null,
    t.quantization,
    t.format,
  ].filter(Boolean).join(" · ");
}

// ------------------------------------------------------------------ results

function SuiteView({ ctx, id, onDeleted }: { ctx: LabCtx; id: string; onDeleted: () => void }) {
  const suite = useAsync(() => api.labBenchGet(id), [id]);
  const [open, setOpen] = useState<number | null>(null);
  const [shownOutput, setShownOutput] = useState<number | null>(null);
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  const [csv, setCsv] = useState("");
  useEffect(() => {
    const sub = listen<string>(LAB_BENCH, (e) => {
      if (e.payload === id) void suite.reload();
    });
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  const s = suite.data;
  const status = s?.status;
  useEffect(() => {
    if (status && status !== "running") api.labBenchExportText(id, "csv").then(setCsv).catch(() => setCsv(""));
  }, [id, status]);
  if (!s) return suite.error ? <div className="p-5"><ErrorBanner message={suite.error} /></div> : <Spinner />;

  const done = s.runs.length;
  const running = s.status === "running";
  const exportTo = async (format: "json" | "csv") => {
    setError(undefined);
    setMessage(undefined);
    try {
      const path = await api.labBenchExport(id, format);
      if (path) setMessage(`Saved to ${path}`);
    } catch (e) {
      setError(errorText(e));
    }
  };
  const anyDerived = s.summary.some((x) => x.tps_source === "derived" || x.tps_source === "mixed");

  return (
    <div data-testid="bench-suite" data-status={s.status} className="mx-auto w-full max-w-5xl space-y-4 p-5">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="selectable min-w-0 flex-1 truncate text-md font-semibold">{s.name}</h2>
        <Badge tone={STATUS_TONE[s.status]}>{s.status}</Badge>
        {running ? (
          <Button size="sm" variant="danger" onClick={() => void api.labBenchCancel(id)}><Square size={11} /> Cancel</Button>
        ) : (
          <>
            <Button size="sm" onClick={() => ctx.go("compare", { suiteId: id })} title="Compare the models of this benchmark side by side"><GitCompareArrows size={12} /> Compare</Button>
            <Button size="sm" onClick={() => void exportTo("json")}><DownloadIcon size={12} /> JSON</Button>
            <Button size="sm" onClick={() => void exportTo("csv")}><DownloadIcon size={12} /> CSV</Button>
            <CopyButton text={csv} label="Copy CSV" />
            <ConfirmButton label="Delete" confirm="Delete this benchmark" icon={<Trash2 size={12} />} onConfirm={async () => { await api.labBenchDelete(id).catch((e) => setError(errorText(e))); onDeleted(); }} />
          </>
        )}
      </div>
      {running && (
        <div>
          <Progress value={percent(done, s.planned_runs)} />
          <div data-testid="bench-progress" className="mt-1 text-xs text-dim">{done} of {s.planned_runs} requests done. Models are tested one at a time.</div>
        </div>
      )}
      <ErrorBanner message={error ?? s.error} />
      {message && <div className="selectable text-xs text-accent">{message}</div>}

      <Card className="p-3">
        <div className="text-xs font-semibold">Test conditions</div>
        <dl data-testid="bench-conditions" className="selectable mt-1.5 space-y-1 text-xs text-dim">
          <div><span className="text-faint">Machine:</span> {hardwareLine(s.hardware)}</div>
          <div><span className="text-faint">Settings:</span> {describeParams(s.config.params)}{s.config.system ? " · with a system prompt" : ""}</div>
          <div>
            <span className="text-faint">Method:</span> {s.config.prompts.length} prompt{s.config.prompts.length === 1 ? "" : "s"} × {s.config.repeats} run{s.config.repeats === 1 ? "" : "s"} per model, one request at a time
            {s.config.warmup ? ", after one unmeasured warm-up request" : ", no warm-up"}{s.config.cold_start ? ", model unloaded first where the runtime allows" : ""}
          </div>
          <div><span className="text-faint">Run:</span> {new Date(s.created_at).toLocaleString()} ({timeAgo(s.created_at)})</div>
        </dl>
        <div className="mt-2 text-2xs leading-relaxed text-faint">
          These figures describe this machine with these settings. They are not comparable with results taken on other hardware, with other settings, or with a different quantization of the same model.
        </div>
      </Card>

      <Card>
        <table data-testid="bench-summary" className="w-full text-sm">
          <thead className="text-left text-xs text-faint">
            <tr className="border-b border-line">
              <th className="px-3 py-2 font-medium">Model</th>
              <th className="font-medium" title="Request sent to first generated text. Measured by NEXUS.">First token</th>
              <th className="font-medium" title="Tokens per second while generating">Speed</th>
              <th className="font-medium" title="Request sent to end of response. Measured by NEXUS.">Total</th>
              <th className="font-medium" title="Time the runtime spent processing the prompt, where it reports it">Prompt</th>
              <th className="font-medium" title="Load time the runtime reported for the first request">Load</th>
              <th className="font-medium" title="Memory the loaded model occupies, where the runtime reports it">Memory</th>
              <th className="px-3 font-medium">Runs</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line/60">
            {s.config.targets.map((t, i) => {
              const x = s.summary[i];
              const runs = s.runs.filter((r) => r.target === i);
              return (
                <Fragment key={i}>
                  <tr data-target={t.model} className="cursor-default hover:bg-raised/40" onClick={() => setOpen(open === i ? null : i)}>
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-1 font-mono text-xs">{open === i ? <ChevronDown size={12} /> : <ChevronRight size={12} />}{t.model}</div>
                      <div className="pl-4 text-2xs text-faint">{targetLine(t)}</div>
                    </td>
                    <td className="pr-3 tabular-nums">{fmtStat(x?.ttft_ms, fmtMs)}</td>
                    <td className="pr-3 tabular-nums">
                      {fmtStat(x?.tokens_per_second, fmtTps)}
                      {x?.tps_source && x.tps_source !== "runtime" && <span className="text-2xs text-warn" title="Derived by NEXUS from the reported output tokens and the client's clock"> {x.tps_source}</span>}
                    </td>
                    <td className="pr-3 tabular-nums">{fmtStat(x?.total_ms, fmtMs)}</td>
                    <td className="pr-3 tabular-nums">{fmtStat(x?.prompt_eval_ms, fmtMs)}</td>
                    <td className="pr-3 tabular-nums">{fmtMs(x?.load_ms)}</td>
                    <td className="pr-3 tabular-nums">{x?.memory_bytes ? fmtBytes(x.memory_bytes) : UNKNOWN}</td>
                    <td className="px-3 tabular-nums">{x?.runs ?? 0}{x?.errors ? <span className="text-danger"> · {x.errors} failed</span> : null}</td>
                  </tr>
                  {open === i && (
                    <tr>
                      <td colSpan={8} className="bg-bg px-3 py-2">
                        <table className="w-full text-xs [&_td]:pr-3 [&_th]:pr-3">
                          <thead className="text-left text-faint">
                            <tr><th className="py-1 font-medium">Prompt</th><th className="font-medium">Run</th><th className="font-medium">First token</th><th className="font-medium">Total</th><th className="font-medium">Speed</th><th className="font-medium">In</th><th className="font-medium">Out</th><th className="font-medium">Load</th><th className="font-medium">Result</th></tr>
                          </thead>
                          <tbody>
                            {runs.map((r) => (
                              <Fragment key={r.id}>
                                <tr data-run={r.status} className="cursor-default border-t border-line/40 hover:bg-raised/40" onClick={() => setShownOutput(shownOutput === r.id ? null : r.id)}>
                                  <td className="py-1">{s.config.prompts[r.prompt]?.label}</td>
                                  <td>{r.warmup ? <span className="text-faint">warm-up</span> : r.iteration}</td>
                                  <td className="tabular-nums">{fmtMs(r.metrics.ttft_ms)}</td>
                                  <td className="tabular-nums">{r.status === "ok" ? fmtMs(r.metrics.total_ms) : UNKNOWN}</td>
                                  <td className="tabular-nums">{fmtTps(r.metrics.tokens_per_second)}</td>
                                  <td className="tabular-nums">{fmtInt(r.metrics.input_tokens)}</td>
                                  <td className="tabular-nums">{fmtInt(r.metrics.output_tokens)}</td>
                                  <td className="tabular-nums">{fmtMs(r.metrics.load_ms)}</td>
                                  <td className={clsx(r.status === "error" && "text-danger", r.status === "cancelled" && "text-faint")}>{r.status === "ok" ? "ok" : r.status === "error" ? (r.error ?? "failed") : "cancelled"}</td>
                                </tr>
                                {shownOutput === r.id && (
                                  <tr><td colSpan={9} className="pb-2"><pre className="selectable max-h-48 overflow-auto rounded border border-line bg-panel p-2 font-mono text-2xs leading-5 whitespace-pre-wrap text-dim">{r.output || "(no output recorded)"}</pre></td></tr>
                                )}
                              </Fragment>
                            ))}
                            {runs.length === 0 && <tr><td colSpan={9} className="py-2 text-faint">No requests have run for this model yet.</td></tr>}
                          </tbody>
                        </table>
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </Card>
      <div className="text-2xs leading-relaxed text-faint">
        Each cell is the median, with the range and number of measured runs. Warm-up requests are listed but not counted. A dash means the runtime did not report the value.
        {anyDerived && " Speeds marked derived were computed by NEXUS from the runtime's reported output tokens and the time after the first token, because that runtime does not time its own generation."}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ new benchmark

function unionKind(ctx: LabCtx, targets: BenchTarget[]): KindInfo | undefined {
  const kinds = targets.map((t) => ctx.kinds.find((k) => k.kind === ctx.providers.find((p) => p.id === t.provider_id)?.kind)).filter((k): k is KindInfo => !!k);
  if (kinds.length === 0) return undefined;
  return { ...kinds[0], params: [...new Set(kinds.flatMap((k) => k.params))] };
}

function NewBenchmark({ ctx, target, onStarted }: { ctx: LabCtx; target: LabTarget | undefined; onStarted: (id: string) => void }) {
  const providers = usable(ctx.providers);
  const first = (): BenchTarget => {
    const p = providers.find((x) => x.id === (target?.providerId ?? ctx.settings?.default_provider_id)) ?? providers[0];
    const model = target?.providerId ? (target.model ?? "") : (p?.id === ctx.settings?.default_provider_id ? ctx.settings?.default_model : null) ?? p?.default_model ?? "";
    return { provider_id: p?.id ?? "", model: model ?? "" };
  };
  const [targets, setTargets] = useState<BenchTarget[]>(() => [first()]);
  const [prompts, setPrompts] = useState<BenchPrompt[]>([PRESETS[0], PRESETS[1]]);
  const [name, setName] = useState("");
  const [repeats, setRepeats] = useState("3");
  const [warmup, setWarmup] = useState(true);
  const [coldStart, setColdStart] = useState(false);
  const [params, setParams] = useState<GenParams>({ ...emptyParams(), temperature: 0, max_tokens: 128, seed: 1 });
  const [paramError, setParamError] = useState<string>();
  const [system, setSystem] = useState("");
  const [allowRemote, setAllowRemote] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (target?.providerId) setTargets([{ provider_id: target.providerId, model: target.model ?? "" }]);
  }, [target]);

  const kind = useMemo(() => unionKind(ctx, targets), [ctx, targets]);
  const chosen = targets.map((t) => providers.find((p) => p.id === t.provider_id));
  const remote = chosen.filter((p) => p && !p.local);
  const repeatCount = Number(repeats);
  const planned = targets.length * (prompts.length * (Number.isInteger(repeatCount) ? repeatCount : 0) + (warmup ? 1 : 0));
  // Settings some chosen runtime will not receive, which matters when comparing.
  const uneven = (kind?.params ?? []).filter((name) => params[name] !== null && chosen.some((p) => p && !ctx.kinds.find((k) => k.kind === p.kind)?.params.includes(name)));
  const hasOllama = chosen.some((p) => p?.kind === "ollama");
  const problem =
    targets.some((t) => !t.provider_id || !t.model.trim()) ? "Every row needs a connection and a model"
      : prompts.some((p) => !p.text.trim()) ? "Every prompt needs some text"
        : !Number.isInteger(repeatCount) || repeatCount < 1 || repeatCount > 20 ? "Repeat each prompt between 1 and 20 times"
          : paramError ?? (remote.length > 0 && !allowRemote ? "Confirm sending the prompts to the remote connection first" : undefined);

  const start = async () => {
    setBusy(true);
    setError(undefined);
    try {
      onStarted(await api.labBenchStart({
        name: name.trim() || null, targets: targets.map((t) => ({ provider_id: t.provider_id, model: t.model.trim() })), prompts,
        system: system.trim() || null, params, repeats: repeatCount, warmup, cold_start: coldStart && hasOllama, allow_remote: allowRemote,
      }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (providers.length === 0) {
    return (
      <Empty icon={<Gauge size={28} />} title="No runtime is connected">
        <p className="mb-3">A benchmark sends real requests to a connected model and measures them. Nothing is connected and enabled yet.</p>
        <Button variant="primary" onClick={() => ctx.go("runtimes")}>Find runtimes on this machine</Button>
      </Empty>
    );
  }

  return (
    <form data-testid="bench-form" className="mx-auto w-full max-w-3xl space-y-5 p-5" onSubmit={(e) => { e.preventDefault(); void start(); }}>
      <div>
        <h2 className="text-md font-semibold">New benchmark</h2>
        <p className="mt-1 text-sm leading-relaxed text-dim">Every model gets the same prompts and settings, one request at a time, so the measurements can be compared. Nothing here is estimated.</p>
      </div>
      <ErrorBanner message={error} />

      <div className="space-y-2">
        <div className="text-xs font-semibold">Models</div>
        {targets.map((t, i) => (
          <div key={i} data-bench-target={i} className="flex items-center gap-2">
            <ModelPicker ctx={ctx} idPrefix={`bench-${i}`} providerId={t.provider_id} model={t.model} onChange={(provider_id, model) => setTargets((list) => list.map((x, j) => (j === i ? { provider_id, model } : x)))} />
            {targets.length > 1 && <button type="button" aria-label="Remove model" className="grid size-7 place-items-center rounded text-faint hover:bg-raised hover:text-ink" onClick={() => setTargets((list) => list.filter((_, j) => j !== i))}><X size={13} /></button>}
          </div>
        ))}
        {targets.length < 8 && <Button type="button" size="sm" onClick={() => setTargets((list) => [...list, { provider_id: list[list.length - 1]?.provider_id ?? providers[0].id, model: "" }])}><Plus size={12} /> Add a model</Button>}
      </div>

      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold">Prompts</span>
          <span className="ml-auto flex gap-1">
            {PRESETS.map((p) => (
              <Button key={p.label} type="button" size="sm" variant="ghost" disabled={prompts.length >= 10 || prompts.some((x) => x.text === p.text)} onClick={() => setPrompts((list) => [...list, p])}><Plus size={11} /> {p.label}</Button>
            ))}
          </span>
        </div>
        {prompts.map((p, i) => (
          <div key={i} className="space-y-1 rounded-md border border-line p-2">
            <div className="flex items-center gap-2">
              <Input aria-label={`Prompt ${i + 1} label`} className="w-48!" placeholder={`Prompt ${i + 1}`} value={p.label} onChange={(e) => setPrompts((list) => list.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))} />
              <span className="text-2xs text-faint">{p.text.length.toLocaleString()} characters</span>
              {prompts.length > 1 && <button type="button" aria-label="Remove prompt" className="ml-auto grid size-6 place-items-center rounded text-faint hover:bg-raised hover:text-ink" onClick={() => setPrompts((list) => list.filter((_, j) => j !== i))}><X size={12} /></button>}
            </div>
            <textarea
              aria-label={`Prompt ${i + 1} text`}
              rows={2}
              value={p.text}
              onChange={(e) => setPrompts((list) => list.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)))}
              className="w-full resize-y rounded-md border border-line bg-bg p-2 text-xs focus:border-lav/60 focus:outline-none"
            />
          </div>
        ))}
        {prompts.length < 10 && <Button type="button" size="sm" onClick={() => setPrompts((list) => [...list, { label: "", text: "" }])}><Plus size={12} /> Add a prompt</Button>}
      </div>

      <div className="grid grid-cols-2 gap-5">
        <div className="space-y-3">
          <div className="text-xs font-semibold">Method</div>
          <Field label="Runs per prompt" hint="More runs show how much the numbers vary."><Input aria-label="Runs per prompt" className="w-24! font-mono" inputMode="numeric" value={repeats} onChange={(e) => setRepeats(e.target.value)} /></Field>
          <label className="flex items-start gap-2 text-xs text-dim">
            <input type="checkbox" className="mt-0.5 accent-accent" checked={warmup} onChange={(e) => setWarmup(e.target.checked)} />
            <span><span className="text-ink">Warm up first.</span> One unmeasured request per model, so loading the model does not skew the measured runs. Its load time is still reported.</span>
          </label>
          <label className={clsx("flex items-start gap-2 text-xs text-dim", !hasOllama && "opacity-50")}>
            <input type="checkbox" className="mt-0.5 accent-accent" checked={coldStart && hasOllama} disabled={!hasOllama} onChange={(e) => setColdStart(e.target.checked)} />
            <span><span className="text-ink">Measure a cold load.</span> Ask Ollama to unload each model first, so the load time is from cold. Other runtimes do not support this.</span>
          </label>
          <Field label="Name" hint="Optional."><Input aria-label="Benchmark name" value={name} onChange={(e) => setName(e.target.value)} /></Field>
        </div>
        <div className="space-y-2">
          <div className="text-xs font-semibold">Settings for every model</div>
          <ParamsEditor kind={kind} value={params} onChange={setParams} onError={setParamError} />
          {uneven.length > 0 && <div className="text-2xs leading-relaxed text-warn">Not every chosen runtime accepts {uneven.join(", ").replace(/_/g, "-")}; it is only sent where supported, which affects how comparable the results are.</div>}
          <textarea aria-label="Benchmark system prompt" rows={2} value={system} onChange={(e) => setSystem(e.target.value)} placeholder="Optional system prompt, sent to every model" className="w-full resize-y rounded-md border border-line bg-bg p-2 text-xs placeholder:text-faint focus:border-lav/60 focus:outline-none" />
        </div>
      </div>

      {remote.length > 0 && (
        <label data-testid="remote-notice" className="flex items-start gap-2 rounded-md border border-warn/30 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
          <input type="checkbox" className="mt-0.5 accent-warn" checked={allowRemote} onChange={(e) => setAllowRemote(e.target.checked)} />
          <span><span className="font-semibold">This leaves your machine.</span> The benchmark prompts will be sent to {[...new Set(remote.map((p) => hostOf(p!.base_url)))].join(", ")}. A remote service's speed also depends on the network and its load. Tick to allow it.</span>
        </label>
      )}

      <div className="flex items-center gap-3 border-t border-line pt-4">
        <Button type="submit" variant="primary" busy={busy} disabled={!!problem} title={problem}><Play size={12} /> Run benchmark</Button>
        <span data-testid="bench-planned" className="text-xs text-dim">{problem ?? `${planned} requests in total, sent one at a time.`}</span>
      </div>
    </form>
  );
}

export default function BenchmarksTab({ ctx, target }: { ctx: LabCtx; target: LabTarget | undefined }) {
  const list = useAsync(api.labBenchList, []);
  const [selected, setSelected] = useState<string | "new">("new");
  useEffect(() => {
    const sub = listen(LAB_BENCH, () => void list.reload());
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    if (target?.providerId) setSelected("new");
  }, [target]);

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-60 shrink-0 flex-col border-r border-line">
        <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-3">
          <span className="text-xs font-semibold">History</span>
          <Button size="sm" variant={selected === "new" ? "primary" : "ghost"} onClick={() => setSelected("new")}><Plus size={12} /> New</Button>
        </div>
        <div className="flex-1 overflow-auto p-2">
          <ErrorBanner message={list.error} />
          {list.data?.length === 0 && <div className="p-2 text-xs leading-relaxed text-faint">Finished benchmarks are kept here with their settings and the machine they ran on.</div>}
          {list.data?.map((s) => (
            <button key={s.id} data-suite={s.name} onClick={() => setSelected(s.id)} className={clsx("mb-0.5 block w-full rounded-md px-2 py-1.5 text-left", selected === s.id ? "bg-raised" : "hover:bg-raised/60")}>
              <div className="flex items-center gap-1.5">
                <span className={clsx("size-1.5 shrink-0 rounded-full", s.status === "completed" ? "bg-accent" : s.status === "running" ? "animate-pulse bg-lav" : s.status === "failed" ? "bg-danger" : "bg-faint")} />
                <span className="truncate text-xs font-medium">{s.name}</span>
              </div>
              <div className="truncate pl-3 text-2xs text-faint">{s.models.length} model{s.models.length === 1 ? "" : "s"} · {s.runs} requests · {timeAgo(s.created_at)}</div>
            </button>
          ))}
        </div>
      </aside>
      <div className="min-w-0 flex-1 overflow-auto">
        {selected === "new"
          ? <NewBenchmark ctx={ctx} target={target} onStarted={(id) => { void list.reload(); setSelected(id); }} />
          : <SuiteView key={selected} ctx={ctx} id={selected} onDeleted={() => { setSelected("new"); void list.reload(); }} />}
      </div>
    </div>
  );
}
