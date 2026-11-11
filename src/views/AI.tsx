import clsx from "clsx";
import { Bot, Check, KeyRound, Plus, Send, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../App";
import { Badge, Button, Card, Empty, ErrorBanner, Field, Input, PageHeader, Select, Spinner, Tabs } from "../components/ui";
import { api, type ChatMessage, type ModelInfo, type Provider, type ProviderInput, type ProviderKind } from "../lib/api";
import { confirmAction } from "../lib/importProject";
import { errorText, formatBytes, formatNumber, useAsync } from "../lib/util";

type Tab = "chat" | "providers" | "local" | "usage";

const PRESETS: { label: string; kind: ProviderKind; base_url: string; needsKey: boolean }[] = [
  { label: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com", needsKey: true },
  { label: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1", needsKey: true },
  { label: "Ollama (local)", kind: "ollama", base_url: "http://localhost:11434", needsKey: false },
  { label: "LM Studio (local)", kind: "openai", base_url: "http://localhost:1234/v1", needsKey: false },
  { label: "Other OpenAI-compatible", kind: "openai", base_url: "https://", needsKey: false },
];

function ProviderForm({ initial, onSaved, onCancel }: { initial?: Provider; onSaved: () => void; onCancel: () => void }) {
  const [form, setForm] = useState<ProviderInput>(
    initial
      ? { id: initial.id, name: initial.name, kind: initial.kind, base_url: initial.base_url, default_model: initial.default_model ?? "" }
      : { name: PRESETS[0].label, kind: PRESETS[0].kind, base_url: PRESETS[0].base_url, default_model: "" },
  );
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const set = (patch: Partial<ProviderInput>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <Card className="p-4">
      <form
        className="space-y-3"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError(undefined);
          try {
            // Only send the key when the user typed one, so editing never wipes a stored key.
            await api.saveProvider({ ...form, ...(key ? { api_key: key } : {}) });
            onSaved();
          } catch (err) {
            setError(errorText(err));
          } finally {
            setBusy(false);
          }
        }}
      >
        <ErrorBanner message={error} />
        {!initial && (
          <Field label="Type">
            <Select
              onChange={(e) => {
                const p = PRESETS[Number(e.target.value)];
                set({ name: p.label.replace(" (local)", ""), kind: p.kind, base_url: p.base_url });
              }}
            >
              {PRESETS.map((p, i) => <option key={p.label} value={i}>{p.label}</option>)}
            </Select>
          </Field>
        )}
        <div className="grid grid-cols-2 gap-3">
          <Field label="Name"><Input value={form.name} onChange={(e) => set({ name: e.target.value })} /></Field>
          <Field label="Default model" hint="Optional. You can also pick one per chat."><Input className="font-mono" value={form.default_model ?? ""} onChange={(e) => set({ default_model: e.target.value })} /></Field>
        </div>
        <Field label="Base URL"><Input className="font-mono" value={form.base_url} onChange={(e) => set({ base_url: e.target.value })} /></Field>
        <Field
          label="API key"
          hint={`Stored in your operating system's keychain, never in the NEXUS database.${initial?.has_key ? " Leave blank to keep the current key." : ""}`}
        >
          <Input type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder={initial?.has_key ? "••••••••  (saved)" : form.kind === "ollama" ? "Not needed for local models" : ""} />
        </Field>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary" busy={busy}>Save provider</Button>
        </div>
      </form>
    </Card>
  );
}

function Providers({ providers, reload }: { providers: Provider[]; reload: () => void }) {
  const [editing, setEditing] = useState<Provider | "new" | null>(null);
  const [test, setTest] = useState<Record<string, { ok: boolean; text: string } | "busy">>({});

  const runTest = async (p: Provider) => {
    setTest((t) => ({ ...t, [p.id]: "busy" }));
    try {
      const models = await api.listModels(p.id);
      setTest((t) => ({ ...t, [p.id]: { ok: true, text: `Connected · ${models.length} models available` } }));
    } catch (e) {
      setTest((t) => ({ ...t, [p.id]: { ok: false, text: errorText(e) } }));
    }
  };

  return (
    <div className="mx-auto w-full max-w-3xl space-y-3 p-5">
      <p className="text-[13px] leading-relaxed text-dim">
        Providers are optional. Project analysis, search, Git and tasks all work without one. Requests go directly from this machine to the URL you configure.
      </p>
      {providers.map((p) =>
        editing !== "new" && editing?.id === p.id ? (
          <ProviderForm key={p.id} initial={p} onCancel={() => setEditing(null)} onSaved={() => { setEditing(null); reload(); }} />
        ) : (
          <Card key={p.id} className="p-4">
            <div className="flex items-center gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-semibold">{p.name}</span>
                  <Badge>{p.kind === "openai" ? "OpenAI-compatible" : p.kind}</Badge>
                  {p.has_key && <Badge tone="green"><KeyRound size={10} className="mr-1" />key in keychain</Badge>}
                </div>
                <div className="selectable mt-0.5 truncate font-mono text-xs text-faint">{p.base_url}{p.default_model ? ` · ${p.default_model}` : ""}</div>
              </div>
              <Button size="sm" busy={test[p.id] === "busy"} onClick={() => runTest(p)}>Test</Button>
              <Button size="sm" onClick={() => setEditing(p)}>Edit</Button>
              <Button
                size="sm"
                variant="ghost"
                title="Delete provider"
                onClick={async () => {
                  if (await confirmAction(`Delete the provider “${p.name}” and its stored API key?`)) {
                    await api.deleteProvider(p.id);
                    reload();
                  }
                }}
              >
                <Trash2 size={13} />
              </Button>
            </div>
            {test[p.id] && test[p.id] !== "busy" && (
              <div className={clsx("selectable mt-2 text-xs", (test[p.id] as { ok: boolean }).ok ? "text-emerald-300" : "text-red-300")}>
                {(test[p.id] as { text: string }).text}
              </div>
            )}
          </Card>
        ),
      )}
      {editing === "new" ? (
        <ProviderForm onCancel={() => setEditing(null)} onSaved={() => { setEditing(null); reload(); }} />
      ) : (
        <Button onClick={() => setEditing("new")}><Plus size={13} /> Add provider</Button>
      )}
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
                  <span className="flex items-center gap-1 text-xs text-emerald-300"><Check size={12} /> added as provider</span>
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

function Chat({ ws, providers, goProviders }: { ws: Workspace; providers: Provider[]; goProviders: () => void }) {
  const [providerId, setProviderId] = useState(providers[0]?.id ?? "");
  const provider = providers.find((p) => p.id === providerId) ?? providers[0];
  const [model, setModel] = useState("");
  const [useProject, setUseProject] = useState(true);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string>();
  const models = useAsync(() => (provider ? api.listModels(provider.id) : Promise.resolve([])), [provider?.id]);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => setModel(provider?.default_model ?? ""), [provider?.id, provider?.default_model]);
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
        <Select className="w-44" value={provider.id} onChange={(e) => setProviderId(e.target.value)} aria-label="Provider">
          {providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </Select>
        <Input className="w-64 font-mono" list="nexus-models" placeholder={models.loading ? "Loading models…" : "Model"} value={model} onChange={(e) => setModel(e.target.value)} aria-label="Model" />
        <datalist id="nexus-models">{models.data?.map((m) => <option key={m.id} value={m.id} />)}</datalist>
        {ws.project && (
          <label className="ml-2 flex items-center gap-1.5 text-xs text-dim">
            <input type="checkbox" checked={useProject} onChange={(e) => setUseProject(e.target.checked)} className="accent-emerald-400" />
            Include summary of {ws.project.name}
          </label>
        )}
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
            className="flex-1 resize-none rounded-md border border-line bg-bg p-2.5 text-[13px] placeholder:text-faint focus:border-accent/60 focus:outline-none"
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
