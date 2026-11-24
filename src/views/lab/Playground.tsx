// Inference Playground: run a prompt against a connected model, watch it stream, stop it, and
// see what was measured. Sessions can be saved and reopened.
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { ChevronDown, ChevronRight, FlaskConical, Plus, Save, Send, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, Empty, ErrorBanner, Input } from "../../components/ui";
import { api, LAB_INFER, type GenParams, type InferEvent, type LabSessionSummary, type LabTurn } from "../../lib/api";
import { describeParams, emptyParams, fmtMs, newRequestId, normalizeParams, paramsFor } from "../../lib/lab";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { ConfirmButton, CopyButton, kindOf, LocalBadge, MetricsLine, ModelPicker, ParamsEditor, RemoteNotice, usable, type LabCtx, type LabTarget } from "./shared";

interface Running { id: string; text: string; thinking: string; startedAt: number; firstAt: number | null }

function Thinking({ text, live }: { text: string; live?: boolean }) {
  const [open, setOpen] = useState(false);
  if (!text) return null;
  return (
    <div className="mb-2">
      <button className="flex items-center gap-1 text-2xs text-faint hover:text-dim" onClick={() => setOpen((o) => !o)}>
        {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />} {live ? "Thinking…" : "Thinking"} ({text.length.toLocaleString()} characters)
      </button>
      {open && <div className="selectable mt-1 border-l-2 border-line pl-3 text-xs leading-relaxed whitespace-pre-wrap text-dim">{text}</div>}
    </div>
  );
}

export default function PlaygroundTab({ ctx, target }: { ctx: LabCtx; target: LabTarget | undefined }) {
  const providers = usable(ctx.providers);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [system, setSystem] = useState("");
  const [showSystem, setShowSystem] = useState(false);
  const [params, setParams] = useState<GenParams>(emptyParams());
  const [paramError, setParamError] = useState<string>();
  const [turns, setTurns] = useState<LabTurn[]>([]);
  const [draft, setDraft] = useState("");
  const [running, setRunning] = useState<Running | null>(null);
  const [allowRemote, setAllowRemote] = useState(false);
  const [error, setError] = useState<string>();
  const [sessionId, setSessionId] = useState<string>();
  const [title, setTitle] = useState("");
  const [saved, setSaved] = useState(true);
  const [now, setNow] = useState(Date.now());
  const sessions = useAsync(api.labSessions, []);
  const runningRef = useRef<Running | null>(null);
  const turnsRef = useRef<LabTurn[]>([]);
  const end = useRef<HTMLDivElement>(null);
  const initialised = useRef(false);
  runningRef.current = running;
  turnsRef.current = turns;

  const provider = providers.find((p) => p.id === providerId);
  const kind = kindOf(ctx, provider);

  // Start from the default model once the settings have loaded.
  useEffect(() => {
    if (initialised.current || !ctx.settings) return;
    initialised.current = true;
    setParams(normalizeParams(ctx.settings.params));
    if (target?.providerId) return;
    const preferred = providers.find((p) => p.id === ctx.settings?.default_provider_id) ?? providers[0];
    if (preferred) {
      setProviderId(preferred.id);
      setModel((preferred.id === ctx.settings.default_provider_id ? ctx.settings.default_model : null) ?? preferred.default_model ?? "");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.settings, providers.length]);

  // Another tab asked to open a specific model here.
  useEffect(() => {
    if (!target?.providerId) return;
    setProviderId(target.providerId);
    setModel(target.model ?? "");
    setAllowRemote(false);
  }, [target]);

  useEffect(() => {
    const sub = listen<InferEvent>(LAB_INFER, (e) => {
      const current = runningRef.current;
      const ev = e.payload;
      if (!current || ev.id !== current.id) return;
      if (ev.kind === "delta") {
        setRunning((r) => r && r.id === ev.id ? { ...r, text: r.text + (ev.text ?? ""), thinking: r.thinking + (ev.thinking ?? ""), firstAt: r.firstAt ?? Date.now() } : r);
      } else if (ev.kind === "done" && ev.outcome) {
        const o = ev.outcome;
        const turn: LabTurn = { role: "assistant", content: o.text, thinking: o.thinking || undefined, metrics: o.metrics, model: o.model, provider_name: o.provider_name, runtime: o.runtime, local: o.local, cancelled: o.cancelled || undefined };
        finish([...turnsRef.current, turn]);
      } else if (ev.kind === "error") {
        finish([...turnsRef.current, { role: "assistant", content: current.text, thinking: current.thinking || undefined, error: ev.message ?? "The request failed" }]);
      }
    });
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => setNow(Date.now()), 200);
    return () => clearInterval(timer);
  }, [running?.id]);
  useEffect(() => end.current?.scrollIntoView({ block: "end" }), [turns.length, running?.text.length, running?.thinking.length]);

  const sessionRef = useRef<{ id?: string; title: string; providerId: string; model: string; system: string; params: GenParams }>({ title: "", providerId: "", model: "", system: "", params });
  sessionRef.current = { id: sessionId, title, providerId, model, system, params };

  const persist = async (list: LabTurn[], explicit: boolean) => {
    const s = sessionRef.current;
    // A session is saved when the user asks, and kept up to date after that.
    if (!explicit && !s.id) return setSaved(false);
    if (list.length === 0) return;
    try {
      const result = await api.labSessionSave({ id: s.id, title: s.title || undefined, provider_id: s.providerId || null, model: s.model || null, system: s.system, params: s.params, messages: list });
      setSessionId(result.id);
      setTitle(result.title);
      setSaved(true);
      void sessions.reload();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const finish = (list: LabTurn[]) => {
    setTurns(list);
    setRunning(null);
    void persist(list, false);
  };

  const send = async () => {
    const content = draft.trim();
    if (!content || running || !provider || !model.trim()) return;
    setError(undefined);
    // Failed turns are shown but not sent back to the model.
    const history = turns.filter((t) => !t.error && (t.role === "user" || t.content.trim() !== ""));
    const next: LabTurn[] = [...turns, { role: "user", content }];
    const id = newRequestId();
    setTurns(next);
    setDraft("");
    setRunning({ id, text: "", thinking: "", startedAt: Date.now(), firstAt: null });
    try {
      await api.labInferStart({
        id,
        provider_id: provider.id,
        model: model.trim(),
        system: system.trim() || null,
        messages: [...history.map(({ role, content }) => ({ role, content })), { role: "user", content }],
        params: paramsFor(params, kind?.params ?? []),
        allow_remote: allowRemote,
      });
    } catch (e) {
      setRunning(null);
      setTurns(turns);
      setDraft(content);
      setError(errorText(e));
    }
  };

  const reset = () => {
    setTurns([]);
    setSessionId(undefined);
    setTitle("");
    setSaved(true);
    setError(undefined);
  };

  const open = async (s: LabSessionSummary) => {
    if (running) return;
    setError(undefined);
    try {
      const full = await api.labSessionGet(s.id);
      setTurns(full.messages);
      setSystem(full.system);
      setShowSystem(full.system.trim() !== "");
      setParams(normalizeParams(full.params));
      setSessionId(full.id);
      setTitle(full.title);
      setSaved(true);
      if (full.provider_id && providers.some((p) => p.id === full.provider_id)) {
        setProviderId(full.provider_id);
        setModel(full.model ?? "");
      } else if (full.provider_id) {
        setError("The connection this session used is no longer available. Its conversation is shown; choose another connection to continue.");
      }
      setAllowRemote(false);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const blocked = useMemo(() => {
    if (providers.length === 0) return "Connect a runtime first";
    if (!provider) return "Choose a connection";
    if (!model.trim()) return "Choose a model";
    if (paramError) return paramError;
    if (!provider.local && !allowRemote) return "Confirm sending to the remote connection first";
    return undefined;
  }, [providers.length, provider, model, paramError, allowRemote]);

  if (providers.length === 0) {
    return (
      <Empty icon={<FlaskConical size={28} />} title="No runtime is connected">
        <p className="mb-3">The Playground runs prompts against a model you have connected. Nothing is connected and enabled yet.</p>
        <div className="flex justify-center gap-2">
          <Button variant="primary" onClick={() => ctx.go("runtimes")}>Find runtimes on this machine</Button>
          <Button onClick={() => ctx.go("providers")}>Add a connection</Button>
        </div>
      </Empty>
    );
  }

  const elapsed = running ? now - running.startedAt : 0;
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-56 shrink-0 flex-col border-r border-line">
        <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-3">
          <span className="text-xs font-semibold">Sessions</span>
          <Button size="sm" variant="ghost" disabled={!!running} onClick={reset} title="Start a new, unsaved session"><Plus size={12} /> New</Button>
        </div>
        <div className="flex-1 overflow-auto p-2">
          {sessions.data?.length === 0 && <div className="p-2 text-xs leading-relaxed text-faint">Saved sessions appear here. Save a session to keep its prompts, outputs and measurements.</div>}
          {sessions.data?.map((s) => (
            <div key={s.id} data-session={s.title} className={clsx("group mb-0.5 flex items-start gap-1 rounded-md px-2 py-1.5", sessionId === s.id ? "bg-raised" : "hover:bg-raised/60")}>
              <button className="min-w-0 flex-1 text-left" onClick={() => void open(s)} disabled={!!running}>
                <div className="truncate text-xs font-medium">{s.title}</div>
                <div className="truncate text-2xs text-faint">{s.model ?? "no model"} · {s.turns} turns · {timeAgo(s.updated_at)}</div>
              </button>
              <button
                className="mt-0.5 hidden text-faint hover:text-danger group-hover:block"
                title="Delete this saved session"
                aria-label={`Delete session ${s.title}`}
                onClick={async () => {
                  await api.labSessionDelete(s.id).catch((e) => setError(errorText(e)));
                  if (sessionId === s.id) reset();
                  void sessions.reload();
                }}
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-line px-4 py-2">
          <ModelPicker ctx={ctx} idPrefix="playground" providerId={providerId} model={model} disabled={!!running} onChange={(p, m) => { if (p !== providerId) setAllowRemote(false); setProviderId(p); setModel(m); }} />
          {provider && <LocalBadge provider={provider} />}
          <span className="ml-auto flex items-center gap-2 text-2xs text-faint">
            {turns.length > 0 && (saved ? (sessionId ? "saved" : "") : "not saved")}
          </span>
        </div>

        <div className="min-h-0 flex-1 overflow-auto">
          <div className="mx-auto max-w-3xl space-y-4 p-5">
            {turns.length === 0 && !running && (
              <div className="pt-10 text-center text-sm leading-relaxed text-faint">
                Enter a prompt to run it against <span className="font-mono text-dim">{model || "a model"}</span>.
                Timings are measured here; token counts come from the runtime and show as a dash when it reports none.
              </div>
            )}
            {turns.map((t, i) => (
              <div key={i} data-turn={t.role} className={clsx("flex", t.role === "user" && "justify-end")}>
                <div className={clsx("max-w-[88%] rounded-lg px-3.5 py-2.5", t.role === "user" ? "bg-raised" : "w-full border border-line bg-panel")}>
                  {t.role === "assistant" && <Thinking text={t.thinking ?? ""} />}
                  <div className="selectable text-sm leading-relaxed break-words whitespace-pre-wrap">{t.content || (t.role === "assistant" && !t.error ? <span className="text-faint">(no text was generated)</span> : null)}</div>
                  {t.error && <div className="selectable mt-2 text-xs text-danger">{t.error}</div>}
                  {t.role === "assistant" && t.metrics && (
                    <div className="mt-2 border-t border-line/60 pt-2">
                      <MetricsLine metrics={t.metrics} />
                      <div className="mt-1 flex items-center gap-2 text-2xs text-faint">
                        <span className="font-mono">{t.model}</span>
                        <span>· {t.provider_name} ({t.runtime})</span>
                        <span>· {t.local ? "ran on this machine" : "ran remotely"}</span>
                        {t.cancelled && <Badge tone="amber">stopped</Badge>}
                        {t.metrics.stop_reason === "refusal" && <Badge tone="amber">the model declined</Badge>}
                        {t.metrics.stop_reason === "length" || t.metrics.stop_reason === "max_tokens" ? <Badge tone="amber">hit the output limit</Badge> : null}
                        <span className="ml-auto"><CopyButton text={t.content} /></span>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            ))}
            {running && (
              <div data-testid="running" className="w-full rounded-lg border border-accent/30 bg-panel px-3.5 py-2.5">
                <Thinking text={running.thinking} live />
                <div className="selectable text-sm leading-relaxed break-words whitespace-pre-wrap">{running.text}<span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-accent align-middle" /></div>
                <div className="mt-2 flex items-center gap-3 border-t border-line/60 pt-2 text-2xs text-dim">
                  <span data-testid="run-status">
                    {running.firstAt === null ? "Waiting for the first token (a model that is not loaded yet takes longer)" : "Generating"} · {fmtMs(elapsed)}
                  </span>
                  {running.firstAt !== null && <span className="text-faint">first text after about {fmtMs(running.firstAt - running.startedAt)}</span>}
                </div>
              </div>
            )}
            <ErrorBanner message={error} />
            <div ref={end} />
          </div>
        </div>

        <form className="shrink-0 space-y-2 border-t border-line p-3" onSubmit={(e) => { e.preventDefault(); void send(); }}>
          <div className="mx-auto max-w-3xl space-y-2">
            <RemoteNotice provider={provider} what="Your prompt and this conversation" confirmed={allowRemote} onConfirm={setAllowRemote} />
            <div className="flex items-end gap-2">
              <textarea
                aria-label="Prompt"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    void send();
                  }
                }}
                rows={3}
                placeholder="Prompt…  (Enter to run, Shift+Enter for a new line)"
                className="flex-1 resize-none rounded-md border border-line bg-bg p-2.5 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none"
              />
              {running ? (
                <Button type="button" variant="danger" onClick={() => void api.labInferCancel(running.id)}><Square size={12} /> Stop</Button>
              ) : (
                <Button type="submit" variant="primary" disabled={!draft.trim() || !!blocked} title={blocked}><Send size={13} /> Run</Button>
              )}
            </div>
            {blocked && draft.trim() && !running && <div className="text-2xs text-warn">{blocked}</div>}
          </div>
        </form>
      </div>

      <aside className="w-72 shrink-0 space-y-4 overflow-auto border-l border-line p-4">
        <div>
          <div className="mb-2 flex items-center justify-between">
            <span className="text-xs font-semibold">Generation settings</span>
            <button className="text-2xs text-faint hover:text-dim" disabled={!!running} onClick={() => setParams(emptyParams())}>reset</button>
          </div>
          <ParamsEditor kind={kind} value={params} onChange={setParams} onError={setParamError} disabled={!!running} />
          <div className="mt-2 text-2xs leading-relaxed text-faint">Sent with the next run: {describeParams(paramsFor(params, kind?.params ?? []))}.</div>
        </div>
        <div>
          <button className="flex items-center gap-1 text-xs font-semibold" onClick={() => setShowSystem((s) => !s)}>
            {showSystem ? <ChevronDown size={12} /> : <ChevronRight size={12} />} System prompt{system.trim() ? " (set)" : ""}
          </button>
          {showSystem && (
            <textarea
              aria-label="System prompt"
              value={system}
              onChange={(e) => setSystem(e.target.value)}
              disabled={!!running}
              rows={5}
              placeholder="Optional instructions sent before the conversation."
              className="mt-2 w-full resize-y rounded-md border border-line bg-bg p-2 text-xs placeholder:text-faint focus:border-lav/60 focus:outline-none"
            />
          )}
          {showSystem && ctx.ws.project?.analysis && (
            <button
              type="button"
              className="mt-1 text-2xs text-accent hover:underline disabled:opacity-40"
              disabled={!!running}
              onClick={() => {
                const p = ctx.ws.project!;
                const a = p.analysis!;
                setSystem(`You are a coding assistant. The user is working on the project "${p.name}".\nSummary: ${a.summary}\nLanguages: ${a.languages.slice(0, 8).map((l) => l.name).join(", ")}\nFrameworks: ${a.frameworks.join(", ") || "none detected"}\nYou cannot read files directly; ask the user to paste code when needed.`);
              }}
            >
              Use a summary of {ctx.ws.project.name} (no file contents)
            </button>
          )}
        </div>
        <div className="space-y-2 border-t border-line/60 pt-3">
          <div className="text-xs font-semibold">Session</div>
          <Input aria-label="Session name" placeholder="Name (defaults to the first prompt)" value={title} onChange={(e) => { setTitle(e.target.value); setSaved(false); }} />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" disabled={turns.length === 0 || !!running} onClick={() => void persist(turns, true)}><Save size={12} /> {sessionId ? "Save changes" : "Save session"}</Button>
            <ConfirmButton label="Clear" confirm="Clear the conversation" disabled={turns.length === 0 || !!running} onConfirm={reset} />
          </div>
          <div className="text-2xs leading-relaxed text-faint">
            Sessions are stored on this machine only. {sessionId ? "This session is updated after every run." : "An unsaved session is lost when you leave the Model Lab."}
          </div>
        </div>
      </aside>
    </div>
  );
}
