// Runtime Manager: which local runtimes are installed and answering, the servers NEXUS started
// itself, what each runtime holds in memory, and how to set one up when it is missing.
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { ChevronDown, ChevronRight, Play, Plus, RefreshCw, Square, TerminalSquare, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Button, Card, ErrorBanner, Field, Input, Select, Spinner } from "../../components/ui";
import { api, LAB_RUNTIME, type Endpoint, type ManagedInfo, type RuntimeStatus } from "../../lib/api";
import { fmtBytes, fmtInt } from "../../lib/lab";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { ConfirmButton, CopyButton, type LabCtx } from "./shared";

const MANAGED_TONE: Record<ManagedInfo["status"], "green" | "amber" | "blue" | "neutral"> = { starting: "blue", running: "green", unresponsive: "amber", exited: "neutral" };

/** Models a runtime is holding in memory right now. Only Ollama reports this. */
function Loaded({ providerId }: { providerId: string }) {
  const loaded = useAsync(() => api.labRuntimeLoaded(providerId), [providerId]);
  const [error, setError] = useState<string>();
  const unload = async (model: string) => {
    setError(undefined);
    try {
      await api.labRuntimeUnload(providerId, model);
    } catch (e) {
      setError(errorText(e));
    }
    await loaded.reload();
  };
  return (
    <div data-testid="loaded-models" className="mt-2 rounded-md border border-line/70 bg-bg p-2">
      <div className="flex items-center gap-2 text-2xs text-faint">
        <span className="font-medium text-dim">In memory now</span>
        <button className="hover:text-ink" onClick={() => void loaded.reload()} title="Refresh">refresh</button>
      </div>
      <ErrorBanner message={error ?? loaded.error} />
      {loaded.data?.length === 0 && <div className="mt-1 text-xs text-faint">No model is loaded. The first request to a model loads it.</div>}
      {loaded.data?.map((m) => (
        <div key={m.name} data-loaded={m.name} className="mt-1 flex items-center gap-3 text-xs">
          <span className="selectable min-w-0 flex-1 truncate font-mono">{m.name}</span>
          <span className="text-dim tabular-nums" title="Memory the loaded model occupies, as reported by the runtime">
            {m.size_bytes !== null ? fmtBytes(m.size_bytes) : "size not reported"}
            {m.size_vram_bytes !== null && m.size_bytes ? ` · ${Math.round((m.size_vram_bytes / m.size_bytes) * 100)}% on GPU` : ""}
          </span>
          {m.context_length !== null && <span className="text-faint tabular-nums">{fmtInt(m.context_length)} ctx</span>}
          {m.expires_at && <span className="text-faint" title={`The runtime will unload it at ${m.expires_at}`}>unloads when idle</span>}
          <Button size="sm" variant="ghost" title="Ask the runtime to release this model's memory now" onClick={() => void unload(m.name)}>Unload</Button>
        </div>
      ))}
    </div>
  );
}

function EndpointRow({ ctx, runtime, ep, reload }: { ctx: LabCtx; runtime: RuntimeStatus; ep: Endpoint; reload: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const connect = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const port = /:(\d+)/.exec(ep.url.replace(/^https?:\/\//, ""))?.[1];
      const taken = ctx.providers.some((p) => p.name === runtime.name.split(" (")[0]);
      await api.saveProvider({ name: taken && port ? `${runtime.name.split(" (")[0]} :${port}` : runtime.name.split(" (")[0], kind: runtime.kind, base_url: ep.url });
      await ctx.reloadProviders();
      await ctx.refreshLibrary();
      await reload();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-endpoint={ep.url} data-running={ep.running} className="rounded-md border border-line p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <span className={clsx("size-2 shrink-0 rounded-full", ep.running ? "bg-accent" : "bg-faint")} />
        <span className="selectable font-mono text-xs">{ep.url}</span>
        <Badge tone={ep.running ? "green" : "neutral"}>{ep.running ? "answering" : ep.provider_enabled === false ? "not checked" : "not answering"}</Badge>
        {ep.version && <span className="text-2xs text-dim">version {ep.version}</span>}
        {ep.models !== null && <span className="text-2xs text-dim">{ep.models} model{ep.models === 1 ? "" : "s"}</span>}
        {ep.detail && <span className="text-2xs text-faint">{ep.detail}</span>}
        {ep.managed_id && <Badge tone="blue">started by NEXUS</Badge>}
        <span className="ml-auto flex items-center gap-2">
          {ep.provider_id ? (
            <span className="text-2xs text-dim">
              connection: <button className="text-accent hover:underline" onClick={() => ctx.go("providers")}>{ep.provider_name}</button>
              {ep.provider_enabled === false && <span className="text-danger"> (disabled)</span>}
            </span>
          ) : ep.running ? (
            <Button size="sm" variant="primary" busy={busy} onClick={() => void connect()}><Plus size={12} /> Add as connection</Button>
          ) : null}
        </span>
      </div>
      <ErrorBanner message={error} />
      {ep.running && ep.provider_id && ep.provider_enabled && runtime.kind === "ollama" && <Loaded providerId={ep.provider_id} />}
    </div>
  );
}

function ManagedRow({ m, reload }: { m: ManagedInfo; reload: () => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [logs, setLogs] = useState<string>();
  const [error, setError] = useState<string>();
  const loadLogs = async () => {
    try {
      setLogs(await api.labRuntimeLogs(m.id));
    } catch (e) {
      setError(errorText(e));
    }
  };
  useEffect(() => {
    if (!open) return;
    void loadLogs();
    if (m.status === "exited") return;
    const timer = setInterval(() => void loadLogs(), 1500);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, m.status]);
  const act = async (fn: () => Promise<unknown>) => {
    setError(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    await reload();
  };
  return (
    <div data-managed={m.id} data-status={m.status} className="rounded-md border border-line p-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={MANAGED_TONE[m.status]}>{m.status === "exited" ? `exited${m.exit_code !== null ? ` · code ${m.exit_code}` : ""}` : m.status}</Badge>
        <span className="selectable font-mono text-xs">{m.url}</span>
        {m.model && <span className="text-2xs text-dim">serving {m.model}</span>}
        {m.pid !== null && <span className="text-2xs text-faint">pid {m.pid}</span>}
        <span className="text-2xs text-faint">started {timeAgo(m.started_at)}</span>
        <span className="ml-auto flex items-center gap-1">
          <Button size="sm" variant="ghost" onClick={() => setOpen((o) => !o)}>{open ? <ChevronDown size={12} /> : <ChevronRight size={12} />} Output</Button>
          {m.status === "exited"
            ? <Button size="sm" variant="ghost" aria-label="Remove from list" onClick={() => void act(() => api.labRuntimeDismiss(m.id))}><X size={12} /></Button>
            : <ConfirmButton label="Stop" confirm="Stop this server" icon={<Square size={11} />} onConfirm={() => act(() => api.labRuntimeStop(m.id))} />}
        </span>
      </div>
      <div className="selectable mt-1 truncate font-mono text-2xs text-faint" title={m.command}>{m.command}</div>
      {m.status === "unresponsive" && <div className="mt-1 text-xs text-warn">The process is running but has not answered on its port. Check its output.</div>}
      <ErrorBanner message={error} />
      {open && <pre data-testid="managed-logs" className="selectable mt-2 max-h-56 overflow-auto rounded-md border border-line bg-bg p-2 font-mono text-2xs leading-5 whitespace-pre-wrap text-dim">{logs === undefined ? "Loading…" : logs || "No output yet."}</pre>}
    </div>
  );
}

function StartForm({ runtime, ggufFiles, defaultPort, reload }: { runtime: RuntimeStatus; ggufFiles: string[]; defaultPort: number; reload: () => Promise<void> }) {
  const [port, setPort] = useState(String(defaultPort));
  const [file, setFile] = useState(ggufFiles[0] ?? "");
  const [context, setContext] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const isLlama = runtime.id === "llamacpp";
  useEffect(() => {
    if (!ggufFiles.includes(file)) setFile(ggufFiles[0] ?? "");
  }, [ggufFiles, file]);

  const start = async () => {
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber < 1024 || portNumber > 65535) return setError("Choose a port between 1024 and 65535");
    const ctxNumber = context.trim() === "" ? null : Number(context);
    if (ctxNumber !== null && (!Number.isInteger(ctxNumber) || ctxNumber < 256)) return setError("Context size must be a whole number of at least 256 tokens");
    setBusy(true);
    setError(undefined);
    try {
      await api.labRuntimeStart({ runtime: runtime.id as "ollama" | "llamacpp", port: portNumber, model_file: isLlama ? file : null, context: isLlama ? ctxNumber : null });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      await reload();
    }
  };

  return (
    <form data-testid={`start-${runtime.id}`} className="space-y-2 rounded-md border border-line p-2.5" onSubmit={(e) => { e.preventDefault(); void start(); }}>
      <div className="text-xs font-medium text-dim">Start a server from NEXUS</div>
      <div className="flex flex-wrap items-end gap-2">
        {isLlama && (
          <Field label="Model file">
            <Select aria-label="Model file" className="w-64!" value={file} onChange={(e) => setFile(e.target.value)} disabled={ggufFiles.length === 0}>
              {ggufFiles.length === 0 && <option value="">No .gguf file in the model folder</option>}
              {ggufFiles.map((f) => <option key={f} value={f}>{f}</option>)}
            </Select>
          </Field>
        )}
        <Field label="Port"><Input aria-label="Port" className="w-24! font-mono" inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value)} /></Field>
        {isLlama && <Field label="Context (tokens)"><Input aria-label="Context size" className="w-32! font-mono" inputMode="numeric" placeholder="model default" value={context} onChange={(e) => setContext(e.target.value)} /></Field>}
        <Button type="submit" size="md" variant="primary" busy={busy} disabled={isLlama && !file}><Play size={12} /> Start</Button>
      </div>
      <div className="text-2xs leading-relaxed text-faint">
        Runs <span className="font-mono">{runtime.installed?.path}</span> bound to 127.0.0.1 only, and stops it when NEXUS closes.
        {isLlama ? " Only files in the model folder can be served." : " It uses Ollama's own model store."}
      </div>
      <ErrorBanner message={error} />
    </form>
  );
}

function RuntimeCard({ ctx, runtime, managed, ggufFiles, reload }: { ctx: LabCtx; runtime: RuntimeStatus; managed: ManagedInfo[]; ggufFiles: string[]; reload: () => Promise<void> }) {
  const anyRunning = runtime.endpoints.some((e) => e.running);
  const [showSetup, setShowSetup] = useState(!anyRunning && !runtime.installed);
  const defaultPort = runtime.id === "ollama" ? (runtime.endpoints.some((e) => e.running && e.url.endsWith(":11434")) ? 11435 : 11434) : 8080;
  return (
    <Card>
      <div data-runtime={runtime.id} className="space-y-3 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-semibold">{runtime.name}</h2>
          {runtime.installed
            ? <Badge tone="green">{runtime.id === "openai" ? "LM Studio tool found" : "installed"}{runtime.installed.version ? ` · ${runtime.installed.version}` : ""}</Badge>
            : <Badge>{runtime.id === "openai" ? "no LM Studio tool found" : "not installed"}</Badge>}
          <Badge tone={anyRunning ? "green" : "neutral"}>{anyRunning ? "running" : "not running"}</Badge>
          {runtime.installed && <span className="selectable truncate font-mono text-2xs text-faint" title="Where the runtime's program was found">{runtime.installed.path}</span>}
        </div>

        <div className="space-y-2">
          {runtime.endpoints.map((ep) => <EndpointRow key={ep.url} ctx={ctx} runtime={runtime} ep={ep} reload={reload} />)}
          {runtime.endpoints.length === 0 && <div className="text-xs text-faint">Nothing is answering on the usual local ports.</div>}
        </div>

        {managed.length > 0 && (
          <div className="space-y-2">
            <div className="text-xs font-medium text-dim">Servers started by NEXUS</div>
            {managed.map((m) => <ManagedRow key={m.id} m={m} reload={reload} />)}
          </div>
        )}
        {runtime.can_start && <StartForm runtime={runtime} ggufFiles={ggufFiles} defaultPort={defaultPort} reload={reload} />}

        <div>
          <button className="flex items-center gap-1 text-xs text-accent hover:underline" onClick={() => setShowSetup((s) => !s)}>
            {showSetup ? <ChevronDown size={12} /> : <ChevronRight size={12} />} Setup instructions
          </button>
          {showSetup && (
            <ol data-testid={`setup-${runtime.id}`} className="mt-2 space-y-2">
              {runtime.setup.map((step, i) => (
                <li key={step.title} className="rounded-md border border-line/70 p-2.5">
                  <div className="text-xs font-medium">{i + 1}. {step.title}</div>
                  <div className="mt-0.5 text-xs leading-relaxed text-dim">{step.detail}</div>
                  {step.url && <div className="selectable mt-1 font-mono text-2xs text-lav">{step.url}</div>}
                  {step.command && (
                    <div className="mt-1.5 flex items-center gap-2">
                      <code className="selectable min-w-0 flex-1 truncate rounded bg-bg px-2 py-1 font-mono text-2xs text-dim">{step.command}</code>
                      <CopyButton text={step.command} />
                    </div>
                  )}
                </li>
              ))}
              <li className="flex items-center gap-2 text-2xs leading-relaxed text-faint">
                NEXUS never runs these commands or installs anything for you. Copy a command and run it in a terminal yourself.
                <Button size="sm" variant="ghost" onClick={() => ctx.ws.go("terminal")}><TerminalSquare size={12} /> Open Terminal</Button>
              </li>
            </ol>
          )}
        </div>
      </div>
    </Card>
  );
}

export default function RuntimesTab({ ctx }: { ctx: LabCtx }) {
  const report = useAsync(api.labRuntimes, []);
  const hardware = useAsync(api.labHardware, []);
  const storage = useAsync(api.labStorage, []);
  const reload = async () => {
    await Promise.all([report.reload(), storage.reload()]);
  };
  useEffect(() => {
    const sub = listen(LAB_RUNTIME, () => void report.reload());
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Connections may have been added or changed on another tab.
  useEffect(() => {
    void report.reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.providers.map((p) => `${p.id}:${p.enabled}:${p.base_url}`).join("|")]);

  const hw = hardware.data;
  const ggufFiles = (storage.data?.files ?? []).filter((f) => !f.partial).map((f) => f.name);
  if (!report.data) return report.error ? <div className="p-5"><ErrorBanner message={report.error} /></div> : <Spinner label="Looking for runtimes on this machine…" />;

  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="mx-auto w-full max-w-4xl space-y-3 p-5">
        <div className="flex items-center gap-3">
          <p className="min-w-0 flex-1 text-sm leading-relaxed text-dim">
            Runtimes on this machine. Detection only looks at the usual install locations and local ports; nothing is installed, started or downloaded unless you ask.
          </p>
          <Button size="sm" busy={report.loading} onClick={() => void reload()}><RefreshCw size={12} /> Rescan</Button>
        </div>
        {hw && (
          <div data-testid="hardware" className="selectable rounded-md border border-line px-3 py-2 text-xs text-dim">
            <span className="text-faint">This machine:</span> {hw.os}{hw.os_version ? ` ${hw.os_version}` : ""} · {hw.arch} · {hw.cpu ?? "CPU not reported"}
            {hw.cores !== null ? ` · ${hw.cores} cores` : ""} · {hw.memory_bytes !== null ? `${fmtBytes(hw.memory_bytes)} memory` : "memory not reported"}
          </div>
        )}
        <ErrorBanner message={report.error} />
        {report.data.runtimes.map((r) => (
          <RuntimeCard key={r.id} ctx={ctx} runtime={r} managed={report.data!.managed.filter((m) => m.runtime === r.id)} ggufFiles={ggufFiles} reload={reload} />
        ))}
      </div>
    </div>
  );
}
