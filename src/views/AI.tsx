import clsx from "clsx";
import { Bot, Check, Plus, Send } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../App";
import { ProviderManager } from "../components/ProviderManager";
import { Badge, Button, Card, Empty, ErrorBanner, Input, PageHeader, Select, Spinner, Tabs } from "../components/ui";
import { api, type ChatMessage, type ModelInfo, type Provider } from "../lib/api";
import { hostOf } from "../lib/lab";
import { errorText, formatBytes, formatNumber, useAsync } from "../lib/util";

type Tab = "chat" | "providers" | "local" | "usage";

function Providers({ providers, reload }: { providers: Provider[]; reload: () => void }) {
  return (
    <div className="mx-auto w-full max-w-3xl p-5">
      <ProviderManager providers={providers} reload={reload} intro="Providers are optional. Project analysis, search, Git and tasks all work without one. Requests go directly from this machine to the URL you configure. Model Lab has the full set of model tools." />
    </div>
  );
}

function ModelTable({ models }: { models: ModelInfo[] }) {
  return (
    <table className="selectable w-full text-[13px]">
      <thead className="text-left text-xs text-faint">
        <tr><th className="px-4 py-2 font-medium">Model</th><th className="font-medium">Family</th><th className="font-medium">Parameters</th><th className="font-medium">Quantization</th><th className="px-4 text-right font-medium">Size</th></tr>
      </thead>
      <tbody className="divide-y divide-line border-t border-line">
        {models.map((m) => (
          <tr key={m.id}>
            <td className="px-4 py-2 font-mono text-xs">{m.id}</td>
            <td className="text-dim">{m.family ?? "—"}</td>
            <td className="text-dim">{m.parameter_size ?? "—"}</td>
            <td className="text-dim">{m.quantization ?? "—"}</td>
            <td className="px-4 text-right text-dim tabular-nums">{m.size_bytes ? formatBytes(m.size_bytes) : "—"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function LocalModels({ providers, reload }: { providers: Provider[]; reload: () => void }) {
  const local = useAsync(api.detectLocal, []);
  if (local.loading && !local.data) return <Spinner label="Looking for local model runtimes…" />;
  return (
    <div className="mx-auto w-full max-w-3xl space-y-3 p-5">
      <ErrorBanner message={local.error} />
      <div className="flex items-center">
        <p className="flex-1 text-[13px] text-dim">Models served by runtimes on this machine. Nothing is sent to the internet.</p>
        <Button size="sm" onClick={() => void local.reload()} busy={local.loading}>Rescan</Button>
      </div>
      {local.data?.map((r) => {
        const added = providers.some((p) => p.base_url === r.base_url);
        return (
          <Card key={r.name}>
            <div className="flex items-center gap-2 px-4 py-3">
              <span className="text-[13px] font-semibold">{r.name}</span>
              <Badge tone={r.running ? "green" : "neutral"}>{r.running ? "running" : "not detected"}</Badge>
              <span className="font-mono text-xs text-faint">{r.base_url}</span>
              <div className="ml-auto">
                {r.running && (added ? (
                  <span className="flex items-center gap-1 text-xs text-accent"><Check size={12} /> added as provider</span>
                ) : (
                  <Button size="sm" onClick={async () => { await api.saveProvider({ name: r.name, kind: r.kind, base_url: r.base_url, default_model: r.models[0]?.id }); reload(); }}>
                    <Plus size={12} /> Use as provider
                  </Button>
                ))}
              </div>
            </div>
            {r.running && (r.models.length ? <ModelTable models={r.models} /> : <div className="border-t border-line px-4 py-3 text-xs text-dim">Running, but no models are installed.</div>)}
          </Card>
        );
      })}
    </div>
  );
}

function Usage() {
  const usage = useAsync(api.usageSummary, []);
  if (!usage.data?.length) return <Empty title="No usage recorded">Token counts appear here after you chat or plan a task with a provider.</Empty>;
  return (
    <div className="mx-auto w-full max-w-3xl p-5">
      <Card>
        <table className="selectable w-full text-[13px]">
          <thead className="text-left text-xs text-faint">
            <tr><th className="px-4 py-2 font-medium">Provider</th><th className="font-medium">Model</th><th className="text-right font-medium">Requests</th><th className="text-right font-medium">Input tokens</th><th className="px-4 text-right font-medium">Output tokens</th></tr>
          </thead>
          <tbody className="divide-y divide-line border-t border-line">
            {usage.data.map((u) => (
              <tr key={u.provider + u.model}>
                <td className="px-4 py-2">{u.provider}</td>
                <td className="font-mono text-xs">{u.model}</td>
                <td className="text-right tabular-nums">{formatNumber(u.requests)}</td>
                <td className="text-right tabular-nums">{formatNumber(u.input_tokens)}</td>
                <td className="px-4 text-right tabular-nums">{formatNumber(u.output_tokens)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

type Turn = ChatMessage & { meta?: string };

function Chat({ ws, providers: allProviders, goProviders }: { ws: Workspace; providers: Provider[]; goProviders: () => void }) {
  const providers = allProviders.filter((p) => p.enabled);
  const defaults = useAsync(api.labSettings, []);
  const [providerId, setProviderId] = useState(providers[0]?.id ?? "");
  const provider = providers.find((p) => p.id === providerId) ?? providers[0];
  const [model, setModel] = useState("");
  // Start on the Model Lab default connection when there is one.
  const defaultProvider = defaults.data?.default_provider_id;
  useEffect(() => {
    if (defaultProvider && providers.some((p) => p.id === defaultProvider)) setProviderId(defaultProvider);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [defaultProvider]);
  const [useProject, setUseProject] = useState(true);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string>();
  const models = useAsync(() => (provider ? api.listModels(provider.id) : Promise.resolve([])), [provider?.id]);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const fromLab = provider && provider.id === defaults.data?.default_provider_id ? defaults.data?.default_model : null;
    setModel(fromLab ?? provider?.default_model ?? "");
  }, [provider?.id, provider?.default_model, defaults.data?.default_provider_id, defaults.data?.default_model]);
  useEffect(() => end.current?.scrollIntoView({ block: "end" }), [turns, busy]);
  useEffect(() => {
    if (!busy) return;
    const started = Date.now();
    const t = setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 500);
    return () => clearInterval(t);
  }, [busy]);

  if (!provider)
    return (
      <Empty icon={<Bot size={30} />} title="No AI provider configured">
        NEXUS works without one. To chat about your code, add a cloud provider or a local runtime such as Ollama.
        <div className="mt-4"><Button variant="primary" onClick={goProviders}>Add a provider</Button></div>
      </Empty>
    );

  const send = async () => {
    const content = draft.trim();
    if (!content || busy) return;
    const history: Turn[] = [...turns, { role: "user", content }];
    setTurns(history);
    setDraft("");
    setBusy(true);
    setElapsed(0);
    setError(undefined);
    const a = useProject ? ws.project?.analysis : null;
    const system = a && ws.project
      ? `You are a coding assistant inside NEXUS, a local developer workspace. The user is working on the project "${ws.project.name}".\nSummary: ${a.summary}\nLanguages: ${a.languages.slice(0, 8).map((l) => l.name).join(", ")}\nFrameworks: ${a.frameworks.join(", ") || "none detected"}\nYou cannot read files directly; ask the user to paste code when needed.`
      : null;
    try {
      const r = await api.chat(provider.id, model || null, system, history.map(({ role, content }) => ({ role, content })));
      const tokens = r.input_tokens !== null || r.output_tokens !== null ? ` · ${formatNumber(r.input_tokens ?? 0)} in / ${formatNumber(r.output_tokens ?? 0)} out tokens` : "";
      setTurns([...history, { role: "assistant", content: r.content, meta: `${r.model} · ${(r.duration_ms / 1000).toFixed(1)}s${tokens}` }]);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2">
        <Select className="w-44!" value={provider.id} onChange={(e) => setProviderId(e.target.value)} aria-label="Provider">
          {providers.map((p) => <option key={p.id} value={p.id}>{p.name}{p.local ? "" : " (remote)"}</option>)}
        </Select>
        <Input className="w-64! font-mono" list="nexus-models" placeholder={models.loading ? "Loading models…" : "Model"} value={model} onChange={(e) => setModel(e.target.value)} aria-label="Model" />
        <datalist id="nexus-models">{models.data?.map((m) => <option key={m.id} value={m.id} />)}</datalist>
        {ws.project && (
          <label className="ml-2 flex items-center gap-1.5 text-xs text-dim">
            <input type="checkbox" checked={useProject} onChange={(e) => setUseProject(e.target.checked)} className="accent-accent" />
            Include summary of {ws.project.name}
          </label>
        )}
        <span data-testid="chat-destination" className={clsx("text-[11px]", provider.local ? "text-faint" : "text-warn")}>
          {provider.local ? "runs on this machine" : `messages are sent to ${hostOf(provider.base_url)}`}
        </span>
        {turns.length > 0 && <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setTurns([])}>Clear chat</Button>}
      </div>
      <div className="flex-1 overflow-auto">
        <div className="mx-auto max-w-3xl space-y-4 p-5">
          {models.error && <ErrorBanner message={`Could not list models: ${models.error}`} />}
          {turns.length === 0 && <div className="pt-16 text-center text-[13px] text-faint">Ask a question about your code, an error message, or how to approach a change.</div>}
          {turns.map((t, i) => (
            <div key={i} className={clsx("flex", t.role === "user" && "justify-end")}>
              <div className={clsx("max-w-[85%] rounded-lg px-3.5 py-2.5", t.role === "user" ? "bg-raised" : "border border-line bg-panel")}>
                <div className="selectable text-[13.5px] leading-relaxed break-words whitespace-pre-wrap">{t.content}</div>
                {t.meta && <div className="mt-2 text-[11px] text-faint">{t.meta}</div>}
              </div>
            </div>
          ))}
          {busy && <div className="flex items-center gap-2 text-xs text-dim"><Spinner label={`Waiting for ${provider.name}… ${elapsed}s`} /></div>}
          <ErrorBanner message={error} />
          <div ref={end} />
        </div>
      </div>
      <form className="shrink-0 border-t border-line p-3" onSubmit={(e) => { e.preventDefault(); void send(); }}>
        <div className="mx-auto flex max-w-3xl items-end gap-2">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void send();
              }
            }}
            rows={2}
            placeholder="Message…  (Enter to send, Shift+Enter for a new line)"
            className="flex-1 resize-none rounded-md border border-line bg-bg p-2.5 text-[13px] placeholder:text-faint focus:border-lav/60 focus:outline-none"
          />
          <Button type="submit" variant="primary" disabled={!draft.trim() || busy}><Send size={13} /> Send</Button>
        </div>
      </form>
    </div>
  );
}

export default function AIView({ ws }: { ws: Workspace }) {
  const [tab, setTab] = useState<Tab>("chat");
  const providers = useAsync(api.listProviders, []);
  const list = providers.data ?? [];
  const reload = () => void providers.reload();
  return (
    <>
      <PageHeader title="AI Workspace" subtitle="Optional. Bring your own provider or run models locally." />
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "chat", label: "Chat" },
          { id: "providers", label: "Providers", count: list.length },
          { id: "local", label: "Local models" },
          { id: "usage", label: "Usage" },
        ]}
      />
      <div className="empty:hidden p-3"><ErrorBanner message={providers.error} /></div>
      {tab === "chat" ? (
        providers.loading && !providers.data ? <Spinner /> : <Chat key={list.map((p) => p.id).join()} ws={ws} providers={list} goProviders={() => setTab("providers")} />
      ) : (
        <div className="flex-1 overflow-auto">
          {tab === "providers" && <Providers providers={list} reload={reload} />}
          {tab === "local" && <LocalModels providers={list} reload={reload} />}
          {tab === "usage" && <Usage />}
        </div>
      )}
    </>
  );
}
