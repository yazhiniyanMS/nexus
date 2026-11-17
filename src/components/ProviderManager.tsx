// Connections to model providers: add, edit, test, disable and remove.
// Used by both the Model Lab (Providers) and the AI Workspace, so there is one place to manage them.
import clsx from "clsx";
import { Globe, HardDrive, KeyRound, Plus, Power, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type ConnectionTest, type KindInfo, type Provider, type ProviderInput, type ProviderKind } from "../lib/api";
import { hostOf } from "../lib/lab";
import { errorText, useAsync } from "../lib/util";
import { Badge, Button, Card, ErrorBanner, Field, Input, Select } from "./ui";

interface Preset { label: string; kind: ProviderKind; base_url: string; name: string; model_hint: string }

const PRESETS: Preset[] = [
  { label: "Ollama (local)", kind: "ollama", base_url: "http://localhost:11434", name: "Ollama", model_hint: "" },
  { label: "llama.cpp server (local)", kind: "llamacpp", base_url: "http://localhost:8080", name: "llama.cpp", model_hint: "" },
  { label: "LM Studio (local)", kind: "openai", base_url: "http://localhost:1234/v1", name: "LM Studio", model_hint: "" },
  { label: "Other OpenAI-compatible endpoint", kind: "openai", base_url: "http://localhost:8000/v1", name: "OpenAI-compatible", model_hint: "" },
  { label: "OpenAI (remote)", kind: "openai", base_url: "https://api.openai.com/v1", name: "OpenAI", model_hint: "" },
  { label: "Anthropic (remote)", kind: "anthropic", base_url: "https://api.anthropic.com", name: "Anthropic", model_hint: "claude-opus-5-5" },
];

const kindLabel = (kinds: KindInfo[], kind: string) => kinds.find((k) => k.kind === kind)?.label ?? kind;

function ProviderForm({ initial, kinds, onSaved, onCancel }: { initial?: Provider; kinds: KindInfo[]; onSaved: () => void; onCancel: () => void }) {
  const [preset, setPreset] = useState(0);
  const [form, setForm] = useState<ProviderInput>(
    initial
      ? { id: initial.id, name: initial.name, kind: initial.kind, base_url: initial.base_url, default_model: initial.default_model ?? "" }
      : { name: PRESETS[0].name, kind: PRESETS[0].kind, base_url: PRESETS[0].base_url, default_model: "" },
  );
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const set = (patch: Partial<ProviderInput>) => setForm((f) => ({ ...f, ...patch }));
  const info = kinds.find((k) => k.kind === form.kind);
  const remote = /^https?:\/\//i.test(form.base_url) && !/^https?:\/\/(localhost|127\.|\[::1\]|0\.0\.0\.0)/i.test(form.base_url);

  return (
    <Card className="p-4">
      <form
        data-testid="provider-form"
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
          <Field label="Type" hint={info?.summary}>
            <Select
              aria-label="Connection type"
              value={preset}
              onChange={(e) => {
                const i = Number(e.target.value);
                setPreset(i);
                set({ name: PRESETS[i].name, kind: PRESETS[i].kind, base_url: PRESETS[i].base_url });
              }}
            >
              {PRESETS.map((p, i) => <option key={p.label} value={i}>{p.label}</option>)}
            </Select>
          </Field>
        )}
        <div className="grid grid-cols-2 gap-3">
          <Field label="Name"><Input aria-label="Name" value={form.name} onChange={(e) => set({ name: e.target.value })} /></Field>
          <Field label="Default model" hint="Optional. Used when a screen does not ask for one.">
            <Input aria-label="Default model" className="font-mono" value={form.default_model ?? ""} placeholder={initial ? "" : PRESETS[preset].model_hint} onChange={(e) => set({ default_model: e.target.value })} />
          </Field>
        </div>
        <Field label="Base URL" hint={remote ? `Requests to this connection leave your machine and go to ${hostOf(form.base_url)}.` : "An address on this machine: requests stay local."}>
          <Input aria-label="Base URL" className="font-mono" value={form.base_url} onChange={(e) => set({ base_url: e.target.value })} />
        </Field>
        <Field
          label="API key"
          hint={`Stored in your operating system's keychain, never in the NEXUS database or its logs.${initial?.has_key ? " Leave blank to keep the current key." : ""}`}
        >
          <Input
            aria-label="API key"
            type="password"
            autoComplete="off"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder={initial?.has_key ? "••••••••  (saved)" : info?.needs_key ? "Required" : "Not needed for local runtimes"}
          />
        </Field>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary" busy={busy}>Save connection</Button>
        </div>
      </form>
    </Card>
  );
}

export function ProviderManager({ providers, reload, intro }: { providers: Provider[]; reload: () => void | Promise<void>; intro?: string }) {
  const kinds = useAsync(api.labKinds, []);
  const [editing, setEditing] = useState<Provider | "new" | null>(null);
  const [tests, setTests] = useState<Record<string, ConnectionTest | "busy">>({});
  const [removing, setRemoving] = useState<string>();
  const [error, setError] = useState<string>();
  const kindList = kinds.data ?? [];

  const act = async (fn: () => Promise<unknown>) => {
    setError(undefined);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const runTest = async (p: Provider) => {
    setTests((t) => ({ ...t, [p.id]: "busy" }));
    try {
      const result = await api.testProvider(p.id);
      setTests((t) => ({ ...t, [p.id]: result }));
    } catch (e) {
      setTests((t) => ({ ...t, [p.id]: { ok: false, latency_ms: null, version: null, models: null, detail: errorText(e) } }));
    }
  };

  return (
    <div className="space-y-3">
      <p className="text-[13px] leading-relaxed text-dim">
        {intro ?? "Connections are optional. Requests go directly from this machine to the address you configure; a connection marked remote sends data off this machine."}
      </p>
      <ErrorBanner message={error ?? kinds.error} />
      {providers.map((p) => {
        const test = tests[p.id];
        if (editing !== "new" && editing?.id === p.id) {
          return <ProviderForm key={p.id} initial={p} kinds={kindList} onCancel={() => setEditing(null)} onSaved={() => { setEditing(null); void reload(); }} />;
        }
        return (
          <Card key={p.id} className={clsx("p-4", !p.enabled && "opacity-70")}>
            <div data-provider={p.name} data-enabled={p.enabled} className="flex items-center gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[13px] font-semibold">{p.name}</span>
                  <Badge>{kindLabel(kindList, p.kind)}</Badge>
                  {p.local
                    ? <Badge tone="green"><HardDrive size={10} className="mr-1" />local</Badge>
                    : <Badge tone="amber"><Globe size={10} className="mr-1" />remote</Badge>}
                  {p.has_key && <Badge><KeyRound size={10} className="mr-1" />key in keychain</Badge>}
                  {!p.enabled && <Badge tone="red">disabled</Badge>}
                </div>
                <div className="selectable mt-0.5 truncate font-mono text-xs text-faint">{p.base_url}{p.default_model ? ` · ${p.default_model}` : ""}</div>
              </div>
              <Button size="sm" busy={test === "busy"} disabled={!p.enabled} title={p.enabled ? "Contact this connection now" : "Enable the connection to test it"} onClick={() => void runTest(p)}>Test</Button>
              <Button size="sm" title={p.enabled ? "Keep the settings but stop using this connection" : "Use this connection again"} onClick={() => void act(() => api.setProviderEnabled(p.id, !p.enabled))}>
                <Power size={12} /> {p.enabled ? "Disable" : "Enable"}
              </Button>
              <Button size="sm" onClick={() => setEditing(p)}>Edit</Button>
              {removing === p.id ? (
                <span className="inline-flex items-center gap-1">
                  <Button size="sm" variant="danger" onClick={() => { setRemoving(undefined); void act(() => api.deleteProvider(p.id)); }}>Remove and delete its key</Button>
                  <Button size="sm" variant="ghost" onClick={() => setRemoving(undefined)}>Keep</Button>
                </span>
              ) : (
                <Button size="sm" variant="ghost" title="Remove connection" aria-label={`Remove ${p.name}`} onClick={() => setRemoving(p.id)}><Trash2 size={13} /></Button>
              )}
            </div>
            {test && test !== "busy" && (
              <div data-testid="provider-test" className={clsx("selectable mt-2 text-xs", test.ok ? "text-accent" : "text-danger")}>
                {test.ok ? `Connected${test.latency_ms !== null ? ` in ${test.latency_ms} ms` : ""} · ${test.detail}` : test.detail}
              </div>
            )}
          </Card>
        );
      })}
      {providers.length === 0 && editing !== "new" && (
        <Card className="p-4 text-[13px] leading-relaxed text-dim">
          No connections yet. If a runtime such as Ollama is running on this machine, Model Lab → Runtimes can add it for you.
        </Card>
      )}
      {editing === "new" ? (
        <ProviderForm kinds={kindList} onCancel={() => setEditing(null)} onSaved={() => { setEditing(null); void reload(); }} />
      ) : (
        <Button onClick={() => setEditing("new")}><Plus size={13} /> Add connection</Button>
      )}
    </div>
  );
}
