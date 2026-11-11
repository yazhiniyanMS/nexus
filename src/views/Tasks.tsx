import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Ban, Check, CircleDashed, ListChecks, Loader2, Play, Plus, RotateCcw, ShieldAlert, SkipForward, Sparkles, Square, Terminal, Trash2, Wand2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../App";
import { Badge, Button, Card, Empty, ErrorBanner, Field, Input, Select } from "../components/ui";
import { api, TASK_CHANGED, TASK_OUTPUT, type Provider, type Step, type StepStatus, type Task, type TaskDetail } from "../lib/api";
import { confirmAction } from "../lib/importProject";
import { errorText, timeAgo, useAsync } from "../lib/util";

const statusTone = { pending: "neutral", running: "blue", awaiting_approval: "amber", completed: "green", failed: "red", cancelled: "neutral", skipped: "neutral" } as const;
const statusText: Record<StepStatus, string> = { pending: "pending", running: "running", awaiting_approval: "needs approval", completed: "completed", failed: "failed", cancelled: "cancelled", skipped: "skipped" };

function StepIcon({ status }: { status: StepStatus }) {
  const map = {
    pending: <CircleDashed size={15} className="text-faint" />,
    running: <Loader2 size={15} className="animate-spin text-sky-400" />,
    awaiting_approval: <ShieldAlert size={15} className="text-amber-400" />,
    completed: <Check size={15} className="text-emerald-400" />,
    failed: <X size={15} className="text-red-400" />,
    cancelled: <Ban size={15} className="text-faint" />,
    skipped: <SkipForward size={15} className="text-faint" />,
  };
  return map[status];
}

function NewTask({ ws, onCreated, onCancel }: { ws: Workspace; onCreated: (t: Task) => void; onCancel: () => void }) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [projectId, setProjectId] = useState(ws.projectId ?? "");
  const [error, setError] = useState<string>();
  return (
    <form
      className="mx-auto w-full max-w-xl space-y-4 p-8"
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          onCreated(await api.createTask(title, description, projectId || null));
        } catch (err) {
          setError(errorText(err));
        }
      }}
    >
      <h2 className="text-[15px] font-semibold">New task</h2>
      <ErrorBanner message={error} />
      <Field label="Title"><Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Upgrade dependencies and verify the build" /></Field>
      <Field label="Details" hint="Optional. Used as context when planning steps with AI.">
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          className="w-full rounded-md border border-line bg-bg p-2.5 text-[13px] placeholder:text-faint focus:border-accent/60 focus:outline-none"
        />
      </Field>
      <Field label="Project" hint="Commands run in this project's folder.">
        <Select value={projectId} onChange={(e) => setProjectId(e.target.value)}>
          <option value="">No project</option>
          {ws.projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </Select>
      </Field>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!title.trim()}>Create task</Button>
      </div>
    </form>
  );
}

function StepRow({ step, live, taskActive, onApprove, onDelete }: {
  step: Step; live: string | undefined; taskActive: boolean; onApprove: (ok: boolean) => void; onDelete: () => void;
}) {
  const output = step.status === "running" ? (live ?? "") : step.output;
  const pre = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (step.status === "running" && pre.current) pre.current.scrollTop = pre.current.scrollHeight;
  }, [output, step.status]);
  const waiting = step.status === "awaiting_approval";

  return (
    <Card className={clsx("overflow-hidden", waiting && "border-amber-500/40")}>
      <div className="flex items-start gap-3 px-4 py-3">
        <div className="pt-0.5"><StepIcon status={step.status} /></div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-medium">{step.position}. {step.title}</span>
            <Badge tone={statusTone[step.status]}>{statusText[step.status]}</Badge>
            {step.risk === "high" && <Badge tone="red">destructive</Badge>}
            {step.exit_code !== null && step.exit_code !== 0 && <span className="text-xs text-red-300">exit {step.exit_code}</span>}
          </div>
          {step.command ? (
            <code className="selectable mt-1.5 flex items-center gap-1.5 font-mono text-xs text-dim"><Terminal size={12} className="shrink-0" /><span className="break-all">{step.command}</span></code>
          ) : (
            <div className="mt-1 text-xs text-faint">Manual step — you do this yourself, then confirm.</div>
          )}
        </div>
        {!taskActive && <button title="Delete step" onClick={onDelete} className="text-faint hover:text-red-300"><Trash2 size={14} /></button>}
      </div>
      {waiting && (
        <div className="flex items-center gap-3 border-t border-amber-500/30 bg-amber-500/5 px-4 py-2.5">
          <div className="min-w-0 flex-1 text-xs text-amber-100">
            {step.kind === "command" ? "NEXUS is waiting for your approval to run this command." : "Confirm once you have completed this step."}
            {step.risk === "high" && (
              <div className="mt-1 flex items-center gap-1.5 font-medium text-red-300">
                <AlertTriangle size={13} /> This command can delete data, rewrite history or use elevated privileges. Read it carefully.
              </div>
            )}
          </div>
          <Button size="sm" onClick={() => onApprove(false)}>Skip</Button>
          <Button size="sm" variant={step.risk === "high" ? "danger" : "primary"} onClick={() => onApprove(true)}>
            {step.kind === "command" ? "Approve & run" : "Mark done"}
          </Button>
        </div>
      )}
      {output && (
        <pre ref={pre} className="selectable max-h-72 overflow-auto border-t border-line bg-bg px-4 py-3 font-mono text-xs leading-5 whitespace-pre-wrap text-dim">{output}</pre>
      )}
    </Card>
  );
}

function Detail({ id, providers, onDeleted, onChanged }: { id: string; providers: Provider[]; onDeleted: () => void; onChanged: () => void }) {
  const [detail, setDetail] = useState<TaskDetail>();
  const [live, setLive] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState<string>();
  const [title, setTitle] = useState("");
  const [command, setCommand] = useState("");
  const [providerId, setProviderId] = useState("");

  const load = () => api.getTask(id).then(setDetail).catch((e) => setError(errorText(e)));
  useEffect(() => {
    void load();
    const subs = [
      listen<string>(TASK_CHANGED, (e) => {
        if (e.payload === id) {
          void load();
          onChanged();
        }
      }),
      listen<{ task_id: string; step_id: string; chunk: string }>(TASK_OUTPUT, (e) => {
        const { task_id, step_id, chunk } = e.payload;
        if (task_id === id) setLive((l) => ({ ...l, [step_id]: ((l[step_id] ?? "") + chunk).slice(-200_000) }));
      }),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const act = async (name: string, fn: () => Promise<unknown>) => {
    setBusy(name);
    setError(undefined);
    try {
      await fn();
      await load();
      onChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(undefined);
    }
  };

  if (!detail) return error ? <div className="p-5"><ErrorBanner message={error} /></div> : null;
  const { task, steps } = detail;
  const active = task.status === "running" || task.status === "awaiting_approval";
  const hasPending = steps.some((s) => s.status === "pending");
  const provider = providers.find((p) => p.id === providerId) ?? providers[0];

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <header className="shrink-0 border-b border-line px-5 py-3">
        <div className="flex items-center gap-3">
          <h1 className="min-w-0 flex-1 truncate text-[15px] font-semibold">{task.title}</h1>
          <Badge tone={statusTone[task.status]}>{statusText[task.status]}</Badge>
          {active ? (
            <Button variant="danger" onClick={() => act("cancel", () => api.cancelTask(id))}><Square size={12} /> Cancel</Button>
          ) : (
            <>
              <Button variant="primary" disabled={!hasPending} busy={busy === "run"} onClick={() => { setLive({}); void act("run", () => api.runTask(id)); }}>
                <Play size={13} /> {steps.some((s) => s.status !== "pending") ? "Continue" : "Run"}
              </Button>
              <Button title="Reset every step to pending" disabled={!steps.length} onClick={() => act("reset", () => api.resetTask(id))}><RotateCcw size={13} /></Button>
              <Button
                variant="ghost"
                title="Delete task"
                onClick={async () => {
                  if (await confirmAction(`Delete the task “${task.title}” and its history?`)) {
                    await api.deleteTask(id).then(onDeleted).catch((e) => setError(errorText(e)));
                  }
                }}
              >
                <Trash2 size={14} />
              </Button>
            </>
          )}
        </div>
        <div className="mt-1 text-xs text-dim">
          {task.project_name ?? "No project"} · {task.steps_done}/{task.step_count} steps done · updated {timeAgo(task.updated_at)}
        </div>
        {task.description && <p className="selectable mt-2 text-[13px] leading-relaxed whitespace-pre-wrap text-dim">{task.description}</p>}
      </header>

      <div className="flex-1 space-y-3 overflow-auto p-5">
        <ErrorBanner message={error} />
        {steps.length === 0 && (
          <Card className="p-5 text-[13px] text-dim">
            This task has no steps yet. Add them by hand, let NEXUS suggest a verification workflow from the project's manifests, or plan them with an AI provider.
          </Card>
        )}
        {steps.map((s) => (
          <StepRow
            key={s.id}
            step={s}
            live={live[s.id]}
            taskActive={active}
            onApprove={(ok) => act("approve", () => api.approveStep(id, s.id, ok))}
            onDelete={() => act("delete", () => api.deleteStep(id, s.id))}
          />
        ))}

        {!active && (
          <Card className="space-y-3 p-4">
            <form
              className="flex items-end gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void act("add", async () => {
                  await api.addStep(id, title, command || null);
                  setTitle("");
                  setCommand("");
                });
              }}
            >
              <div className="flex-1"><Field label="Add a step"><Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="What needs to happen" /></Field></div>
              <div className="flex-1"><Field label="Command (optional)"><Input className="font-mono" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npm test" /></Field></div>
              <Button type="submit" disabled={!title.trim()} busy={busy === "add"}><Plus size={13} /> Add</Button>
            </form>
            <div className="flex flex-wrap items-center gap-2 border-t border-line pt-3">
              <Button size="sm" disabled={!task.project_id} title={task.project_id ? "Derive steps from the project's manifests (no AI)" : "Link the task to a project first"} busy={busy === "suggest"} onClick={() => act("suggest", () => api.suggestSteps(id))}>
                <Wand2 size={12} /> Suggest steps
              </Button>
              {providers.length > 0 ? (
                <>
                  <Button size="sm" busy={busy === "plan"} onClick={() => act("plan", () => api.planWithAi(id, provider.id, null))}>
                    <Sparkles size={12} /> Plan with AI
                  </Button>
                  <Select className="h-7 w-44 text-xs" value={provider.id} onChange={(e) => setProviderId(e.target.value)} aria-label="AI provider">
                    {providers.map((p) => <option key={p.id} value={p.id}>{p.name}{p.default_model ? ` · ${p.default_model}` : ""}</option>)}
                  </Select>
                </>
              ) : (
                <span className="text-xs text-faint">AI planning is available once you add a provider. It is optional.</span>
              )}
            </div>
          </Card>
        )}
      </div>
    </div>
  );
}

export default function TasksView({ ws }: { ws: Workspace }) {
  const tasks = useAsync(api.listTasks, []);
  const providers = useAsync(api.listProviders, []);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const current = selected ?? tasks.data?.[0]?.id ?? null;

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-64 shrink-0 flex-col border-r border-line">
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-line px-4">
          <span className="text-[13px] font-semibold">Tasks</span>
          <Button size="sm" variant="primary" onClick={() => setCreating(true)}><Plus size={13} /> New</Button>
        </div>
        <div className="flex-1 overflow-auto p-2">
          {tasks.data?.map((t) => (
            <button
              key={t.id}
              onClick={() => { setSelected(t.id); setCreating(false); }}
              className={clsx("mb-0.5 block w-full rounded-md px-2.5 py-2 text-left", !creating && t.id === current ? "bg-raised" : "hover:bg-raised/60")}
            >
              <div className="truncate text-[13px] font-medium">{t.title}</div>
              <div className="mt-0.5 flex items-center gap-2 text-xs text-faint">
                <Badge tone={statusTone[t.status]}>{statusText[t.status]}</Badge>
                <span>{t.steps_done}/{t.step_count}</span>
                <span className="ml-auto truncate">{t.project_name}</span>
              </div>
            </button>
          ))}
        </div>
      </aside>
      {creating ? (
        <div className="flex-1 overflow-auto">
          <NewTask ws={ws} onCancel={() => setCreating(false)} onCreated={(t) => { setCreating(false); setSelected(t.id); void tasks.reload(); }} />
        </div>
      ) : current ? (
        <Detail key={current} id={current} providers={providers.data ?? []} onChanged={() => void tasks.reload()} onDeleted={() => { setSelected(null); void tasks.reload(); }} />
      ) : (
        <div className="flex-1">
          <Empty icon={<ListChecks size={30} />} title="No tasks yet">
            A task is a list of steps. Commands never run on their own: each one waits for your approval, shows live output, and can be cancelled.
          </Empty>
        </div>
      )}
    </div>
  );
}
