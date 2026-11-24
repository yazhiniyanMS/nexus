import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, ListChecks, Play, Plus, RotateCcw, Sparkles, Square, Terminal, Trash2, Wand2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Workspace } from "../App";
import { Badge, Button, Card, ConfirmButton, Empty, ErrorBanner, Field, Input, Select, Status, ViewHeader, type StatusKind } from "../components/ui";
import { ExecNote, formatLimit } from "../components/ExecNote";
import { api, TASK_CHANGED, TASK_OUTPUT, type Provider, type Step, type StepStatus, type Task, type TaskDetail } from "../lib/api";
import { errorText, timeAgo, useAsync } from "../lib/util";

const statusKind: Record<StepStatus, StatusKind> = { pending: "idle", running: "running", awaiting_approval: "waiting", completed: "done", failed: "failed", cancelled: "cancelled", skipped: "cancelled" };
const statusText: Partial<Record<StepStatus, string>> = { pending: "Not run", awaiting_approval: "Needs approval", skipped: "Skipped" };

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
      <h2 className="text-md font-semibold">New task</h2>
      <ErrorBanner message={error} />
      <Field label="Title"><Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Upgrade dependencies and verify the build" /></Field>
      <Field label="Details" hint="Optional. Used as context when planning steps with AI.">
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          className="w-full rounded-md border border-line bg-bg p-2.5 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none"
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

function StepRow({ step, live, taskActive, timeoutSecs, onApprove, onDelete }: {
  step: Step; live: string | undefined; taskActive: boolean; timeoutSecs: number | null; onApprove: (ok: boolean) => void; onDelete: () => void;
}) {
  const output = step.status === "running" ? (live ?? "") : step.output;
  const pre = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (step.status === "running" && pre.current) pre.current.scrollTop = pre.current.scrollHeight;
  }, [output, step.status]);
  const waiting = step.status === "awaiting_approval";

  return (
    <div className={clsx("border-b border-line/60", waiting && "border-l-2 border-l-warn bg-warn/[0.04]")}>
      <div className="flex items-start gap-3 px-4 py-3">
        <div className="pt-0.5"><Status kind={statusKind[step.status]} label={statusText[step.status]} showLabel={false} /></div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{step.position}. {step.title}</span>
            <Status kind={statusKind[step.status]} label={statusText[step.status]} />
            {step.risk === "high" && <Badge tone="red">destructive</Badge>}
            {step.exit_code !== null && step.exit_code !== 0 && <span className="text-xs text-danger">exit {step.exit_code}</span>}
          </div>
          {step.command ? (
            <code className="selectable mt-1.5 flex items-center gap-1.5 font-mono text-xs text-dim"><Terminal size={12} className="shrink-0" /><span className="break-all">{step.command}</span></code>
          ) : (
            <div className="mt-1 text-xs text-faint">Manual step — you do this yourself, then confirm.</div>
          )}
        </div>
        {!taskActive && <button title="Delete step" onClick={onDelete} className="text-faint hover:text-danger"><Trash2 size={14} /></button>}
      </div>
      {waiting && (
        <div className="flex items-center gap-3 border-t border-warn/30 bg-warn/5 px-4 py-2.5">
          <div className="min-w-0 flex-1 text-xs text-warn">
            {step.kind === "command" ? "NEXUS is waiting for your approval to run this command." : "Confirm once you have completed this step."}
            {step.kind === "command" && <div className="mt-1"><ExecNote kind="task" timeoutSecs={timeoutSecs} /></div>}
            {step.risk === "high" && (
              <div className="mt-1 flex items-center gap-1.5 font-medium text-danger">
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
    </div>
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
          <h1 className="min-w-0 flex-1 truncate text-md font-semibold">{task.title}</h1>
          <Status kind={statusKind[task.status]} label={statusText[task.status]} />
          {active ? (
            <Button variant="danger" onClick={() => act("cancel", () => api.cancelTask(id))}><Square size={12} /> Cancel</Button>
          ) : (
            <>
              <Button variant="primary" disabled={!hasPending} busy={busy === "run"} onClick={() => { setLive({}); void act("run", () => api.runTask(id)); }}>
                <Play size={13} /> {steps.some((s) => s.status !== "pending") ? "Continue" : "Run"}
              </Button>
              <Button title="Reset every step to pending" disabled={!steps.length} onClick={() => act("reset", () => api.resetTask(id))}><RotateCcw size={13} /></Button>
              <ConfirmButton
                label=""
                icon={<Trash2 size={13} />}
                title="Delete task"
                confirm="Delete task and its history"
                onConfirm={() => api.deleteTask(id).then(onDeleted).catch((e) => setError(errorText(e)))}
              />
            </>
          )}
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-1 text-xs text-dim">
          <span>{task.project_name ?? "No project"} · {task.steps_done}/{task.step_count} steps done · updated {timeAgo(task.updated_at)} ·</span>
          <label className="flex items-center gap-1">
            time limit per step
            <select
              aria-label="Time limit per step"
              className="rounded border border-line bg-bg px-1 py-px text-xs text-ink disabled:opacity-60"
              disabled={active}
              value={task.timeout_secs ?? ""}
              onChange={(e) => act("timeout", () => api.setTaskTimeout(id, e.target.value === "" ? null : Number(e.target.value)))}
            >
              <option value="">default (Settings)</option>
              {[60, 300, 900, 1800, 3600, 7200, 14400, 28800].map((s) => <option key={s} value={s}>{formatLimit(s)}</option>)}
              {task.timeout_secs !== null && ![60, 300, 900, 1800, 3600, 7200, 14400, 28800].includes(task.timeout_secs) && (
                <option value={task.timeout_secs}>{formatLimit(task.timeout_secs)}</option>
              )}
            </select>
          </label>
        </div>
        {task.description && <p className="selectable mt-2 text-sm leading-relaxed whitespace-pre-wrap text-dim">{task.description}</p>}
      </header>

      <div className="flex-1 space-y-3 overflow-auto p-5">
        <ErrorBanner message={error} />
        {steps.length === 0 && (
          <Card className="p-5 text-sm text-dim">
            This task has no steps yet. Add them by hand, let NEXUS suggest a verification workflow from the project's manifests, or plan them with an AI provider.
          </Card>
        )}
        {steps.map((s) => (
          <StepRow
            key={s.id}
            step={s}
            live={live[s.id]}
            taskActive={active}
            timeoutSecs={task.timeout_secs}
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
                  <Select className="h-7! w-44! text-xs" value={provider.id} onChange={(e) => setProviderId(e.target.value)} aria-label="AI provider">
                    {providers.map((p) => <option key={p.id} value={p.id}>{p.name}{p.default_model ? ` · ${p.default_model}` : ""}{p.local ? "" : " (remote)"}</option>)}
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
        <ViewHeader title="Tasks" actions={<Button size="sm" variant="primary" onClick={() => setCreating(true)}><Plus size={13} /> New</Button>} />
        <div className="flex-1 overflow-auto p-2">
          {tasks.data?.map((t) => (
            <button
              key={t.id}
              onClick={() => { setSelected(t.id); setCreating(false); }}
              className={clsx("mb-0.5 flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left", !creating && t.id === current ? "bg-raised" : "hover:bg-raised/60")}
            >
              <Status kind={statusKind[t.status]} label={statusText[t.status]} showLabel={false} className="mt-[3px]" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm">{t.title}</span>
                <span className="block truncate text-2xs text-faint">{t.steps_done} of {t.step_count} steps · {t.project_name ?? "no project"}</span>
              </span>
            </button>
          ))}
        </div>
      </aside>
      {creating ? (
        <div className="flex-1 overflow-auto">
          <NewTask ws={ws} onCancel={() => setCreating(false)} onCreated={(t) => { setCreating(false); setSelected(t.id); void tasks.reload(); }} />
        </div>
      ) : current ? (
        <Detail key={current} id={current} providers={(providers.data ?? []).filter((p) => p.enabled)} onChanged={() => void tasks.reload()} onDeleted={() => { setSelected(null); void tasks.reload(); }} />
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
