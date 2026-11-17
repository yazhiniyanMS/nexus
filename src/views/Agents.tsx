import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Ban, Check, CircleDashed, Eye, FileDiff, Loader2, Pause, Play, Plus, ShieldAlert, SkipForward, Square, Terminal, Trash2, Workflow, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Workspace } from "../App";
import { ModelChoiceControls, ModelChoiceNotice, useModelChoice } from "../components/ModelChoice";
import { Badge, Button, Card, DiffView, Empty, ErrorBanner, Field, Select, Spinner, Tabs } from "../components/ui";
import { AGENT_CHANGED, AGENT_OUTPUT, api, type AgentDiff, type AgentRun, type AgentRunDetail, type AgentRunStatus, type AgentStep, type AgentStepStatus, type Provider, type RoleInfo } from "../lib/api";
import { confirmAction } from "../lib/importProject";
import { errorText, timeAgo, useAsync } from "../lib/util";

type Tone = "neutral" | "green" | "red" | "amber" | "blue";
const runTone: Record<AgentRunStatus, Tone> = { queued: "neutral", running: "blue", paused: "amber", completed: "green", failed: "red", cancelled: "neutral", rejected: "neutral", discarded: "neutral", interrupted: "amber" };
const roleStyle: Record<string, string> = {
  architect: "text-lav border-lav/30 bg-lav/10",
  research: "text-lav border-lav/30 bg-lav/10",
  developer: "text-accent border-accent/30 bg-accent/10",
  testing: "text-warn border-warn/30 bg-warn/10",
  review: "text-danger border-danger/30 bg-danger/10",
  user: "text-ink border-line bg-raised",
  engine: "text-dim border-line bg-raised",
};
const ACTIVE: AgentRunStatus[] = ["queued", "running", "paused", "interrupted", "failed"];

function RoleTag({ role }: { role: string }) {
  return <span className={clsx("inline-flex rounded-full border px-2 py-px text-[11px] font-medium capitalize", roleStyle[role] ?? roleStyle.engine)}>{role === "user" ? "you" : role}</span>;
}

function parse<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

function StepIcon({ status }: { status: AgentStepStatus }) {
  const map: Record<AgentStepStatus, ReactNode> = {
    pending: <CircleDashed size={15} className="text-faint" />,
    running: <Loader2 size={15} className="animate-spin text-lav" />,
    awaiting_approval: <ShieldAlert size={15} className="text-warn" />,
    completed: <Check size={15} className="text-accent" />,
    failed: <X size={15} className="text-danger" />,
    skipped: <SkipForward size={15} className="text-faint" />,
    rejected: <Ban size={15} className="text-faint" />,
    cancelled: <Ban size={15} className="text-faint" />,
  };
  return <>{map[status]}</>;
}

// ------------------------------------------------------------------ overview

function Overview({ roles, runs, providers: allProviders, ws, onNew }: { roles: RoleInfo[]; runs: AgentRun[]; providers: Provider[]; ws: Workspace; onNew: () => void }) {
  // Only connections that are switched on can run agents.
  const providers = allProviders.filter((p) => p.enabled);
  const waiting = runs.reduce((n, r) => n + (r.status === "running" ? r.pending_approvals : 0), 0);
  return (
    <div className="flex-1 overflow-auto">
      <div className="mx-auto max-w-5xl space-y-5 p-6">
        <div className="flex items-end gap-4">
          <div className="flex-1">
            <h1 className="text-[17px] font-semibold">Agent Command Center</h1>
            <p className="mt-1 max-w-2xl text-[13px] leading-relaxed text-dim">
              Describe an outcome. Five specialised agents research, plan, implement, test and review it in an isolated copy of your repository.
              Nothing runs and nothing reaches your project without your approval.
            </p>
          </div>
          <Button variant="primary" onClick={onNew} disabled={providers.length === 0 || ws.projects.length === 0}><Plus size={14} /> New agent task</Button>
        </div>

        {providers.length === 0 && (
          <Card className="border-warn/30 p-4" >
            <div className="flex items-start gap-3" data-testid="agent-setup">
              <AlertTriangle size={16} className="mt-0.5 shrink-0 text-warn" />
              <div className="flex-1 text-[13px] leading-relaxed">
                <div className="font-semibold">Agents need a model</div>
                <div className="mt-0.5 text-dim">
                  {allProviders.length === 0 ? "No model connection is configured" : "Every model connection is disabled"}, so agents cannot run yet.
                  Model Lab finds local runtimes such as Ollama on this machine and lets you add other connections. The rest of NEXUS keeps working without one.
                </div>
              </div>
              <Button onClick={() => ws.go("models")}>Open Model Lab</Button>
            </div>
          </Card>
        )}
        {providers.length > 0 && ws.projects.length === 0 && (
          <Card className="p-4 text-[13px] text-dim">Import a project first. Agents work on a Git repository with at least one commit.</Card>
        )}

        <div className="grid grid-cols-3 gap-3">
          {[["Active tasks", runs.filter((r) => ["queued", "running", "paused"].includes(r.status)).length], ["Waiting for your approval", waiting], ["Completed", runs.filter((r) => r.status === "completed").length]].map(([k, v]) => (
            <Card key={k} className="p-4">
              <div className="text-xs text-dim">{k}</div>
              <div className={clsx("mt-1 text-2xl font-semibold tabular-nums", k === "Waiting for your approval" && Number(v) > 0 && "text-warn")}>{v}</div>
            </Card>
          ))}
        </div>

        <div>
          <div className="mb-2 text-xs font-semibold text-dim">The team</div>
          <div className="grid grid-cols-5 gap-3" data-testid="agent-roles">
            {roles.map((r) => (
              <Card key={r.id} className="flex flex-col p-3.5">
                <RoleTag role={r.id} />
                <ul className="mt-2.5 flex-1 space-y-1 text-xs leading-snug text-dim">
                  {r.responsibilities.map((x) => <li key={x}>{x}</li>)}
                </ul>
                <div className="mt-3 space-y-0.5 border-t border-line pt-2 text-[11px]">
                  {[["Read project", r.can_read], ["Write to workspace", r.can_write_workspace], ["Run approved commands", r.can_run_commands]].map(([label, ok]) => (
                    <div key={String(label)} className={clsx("flex items-center gap-1.5", ok ? "text-ink" : "text-faint line-through")}>
                      {ok ? <Check size={11} className="text-accent" /> : <X size={11} />} {label}
                    </div>
                  ))}
                </div>
              </Card>
            ))}
          </div>
          <p className="mt-2 text-xs text-faint">No agent can write to your project. Only you can, by approving the final diff.</p>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ new task

function NewRun({ ws, providers, onCreated, onCancel }: { ws: Workspace; providers: Provider[]; onCreated: (r: AgentRun) => void; onCancel: () => void }) {
  const [outcome, setOutcome] = useState("");
  const [projectId, setProjectId] = useState(ws.projectId ?? "");
  // Starts on the Model Lab default. The choice is shown and stored with the task; an agent
  // task never changes its model afterwards.
  const choice = useModelChoice(providers, "default");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  return (
    <div className="flex-1 overflow-auto">
      <form
        className="mx-auto w-full max-w-2xl space-y-4 p-8"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError(undefined);
          try {
            onCreated(await api.createAgentRun(projectId, outcome, choice.providerId, choice.model.trim() || null, choice.allowRemote));
          } catch (err) {
            setError(errorText(err));
          } finally {
            setBusy(false);
          }
        }}
      >
        <h2 className="text-[15px] font-semibold">New agent task</h2>
        <ErrorBanner message={error} />
        <Field label="Desired outcome" hint="Describe what should be true when the task is done. The Architect turns this into a plan you approve before anything is written.">
          <textarea
            autoFocus
            aria-label="Desired outcome"
            value={outcome}
            onChange={(e) => setOutcome(e.target.value)}
            rows={5}
            placeholder="e.g. Add a --json flag to the export command that prints the report as JSON, with a test."
            className="w-full rounded-md border border-line bg-bg p-2.5 text-[13px] placeholder:text-faint focus:border-lav/60 focus:outline-none"
          />
        </Field>
        <Field label="Project" hint="Agents work in an isolated copy at the project's current commit. Uncommitted changes are not included and are never modified.">
          <Select aria-label="Project" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
            {ws.projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
        </Field>
        <Field label="Model" hint="Every agent in this task uses exactly this connection and model. It is fixed when the task is created and is never switched automatically.">
          <div className="flex flex-wrap items-center gap-2">
            <ModelChoiceControls choice={choice} providers={providers} idPrefix="agent" disabled={busy} />
          </div>
        </Field>
        <ModelChoiceNotice choice={choice} what="Your instructions and the project code the agents read" />
        <div className="flex items-center justify-end gap-2">
          {choice.problem && choice.providerId && <span className="mr-auto text-xs text-warn">{choice.problem}</span>}
          <Button type="button" variant="ghost" onClick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary" busy={busy} disabled={outcome.trim().length < 8 || !projectId || !choice.providerId || !!choice.problem}>Create task</Button>
        </div>
      </form>
    </div>
  );
}

// ------------------------------------------------------------------ step bodies

interface Plan { analysis: string; issues: string[]; steps: { title: string; detail: string }[]; files: string[]; test_commands: string[] }
interface Research { summary: string; sources: { n: number; path: string; start_line: number; end_line: number }[] }
interface CommandSpec { command: string; source: "discovered" | "ai"; risk: string }
interface Review { verdict: string; summary: string; issues: { severity: string; file: string; note: string }[] }

function PlanBody({ plan }: { plan: Plan }) {
  return (
    <div className="selectable space-y-3 text-[13px] leading-relaxed">
      {plan.analysis && <p className="text-dim">{plan.analysis}</p>}
      {plan.issues.length > 0 && (
        <div>
          <div className="mb-1 text-xs font-semibold text-dim">Issues found</div>
          <ul className="list-disc space-y-0.5 pl-5 text-dim">{plan.issues.map((i, n) => <li key={n}>{i}</li>)}</ul>
        </div>
      )}
      <div>
        <div className="mb-1 text-xs font-semibold text-dim">Plan</div>
        <ol className="list-decimal space-y-1 pl-5">
          {plan.steps.map((s, n) => <li key={n}><span className="font-medium">{s.title}</span>{s.detail && <span className="text-dim"> — {s.detail}</span>}</li>)}
        </ol>
      </div>
      {plan.files.length > 0 && (
        <div>
          <div className="mb-1 text-xs font-semibold text-dim">Files to change</div>
          <div className="flex flex-wrap gap-1.5">{plan.files.map((f) => <span key={f} className="rounded border border-line px-1.5 py-px font-mono text-xs">{f}</span>)}</div>
        </div>
      )}
      {plan.test_commands.length > 0 && (
        <div>
          <div className="mb-1 text-xs font-semibold text-dim">Suggested test commands <span className="font-normal text-faint">(each will ask for approval separately)</span></div>
          {plan.test_commands.map((c) => <code key={c} className="block font-mono text-xs text-dim">{c}</code>)}
        </div>
      )}
    </div>
  );
}

function StepCard({ step, steps, run, live, ws, act, showChanges }: {
  step: AgentStep; steps: AgentStep[]; run: AgentRun; live?: string; ws: Workspace; act: (fn: () => Promise<unknown>) => void; showChanges: () => void;
}) {
  // Decisions can only be taken while the engine is running this task.
  const waiting = step.status === "awaiting_approval" && run.status === "running";
  const decide = (ok: boolean) => act(() => api.agentDecide(run.id, step.id, ok));
  const pre = useRef<HTMLPreElement>(null);
  const output = step.status === "running" ? (live ?? "") : step.output;
  useEffect(() => {
    if (step.status === "running" && pre.current) pre.current.scrollTop = pre.current.scrollHeight;
  }, [output, step.status]);

  let body: ReactNode = null;
  if (step.kind === "research" && step.status === "completed") {
    const r = parse<Research>(step.output);
    body = r && (
      <div className="space-y-2 text-[13px] leading-relaxed">
        <p className="selectable text-dim">{r.summary}</p>
        {r.sources.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {r.sources.map((s) => (
              <button key={s.n} onClick={() => ws.peek({ projectId: run.project_id, path: s.path, line: s.start_line })} className="rounded border border-line bg-raised px-1.5 py-px font-mono text-xs text-lav hover:border-lav/50">
                [{s.n}] {s.path}:{s.start_line}
              </button>
            ))}
          </div>
        )}
      </div>
    );
  } else if (step.kind === "plan" && step.status === "completed") {
    const plan = parse<Plan>(step.output);
    body = plan && <PlanBody plan={plan} />;
  } else if (step.kind === "plan_approval" && waiting) {
    body = (
      <div className="flex items-center gap-3">
        <div className="flex-1 text-xs text-warn">Review the Architect's plan above. Approving lets the Developer write to an isolated workspace — not to your project.</div>
        <Button size="sm" onClick={() => decide(false)}>Reject plan</Button>
        <Button size="sm" variant="primary" onClick={() => decide(true)}>Approve plan</Button>
      </div>
    );
  } else if (step.kind === "develop" && step.status === "completed") {
    const d = parse<{ summary: string; files: AgentDiff["files"] }>(step.output);
    body = d && (
      <div className="space-y-2 text-[13px]">
        <p className="selectable text-dim">{d.summary || "No summary given."}</p>
        {d.files.length === 0 ? <div className="text-xs text-faint">No files were changed.</div> : (
          <button onClick={showChanges} className="flex flex-wrap gap-1.5 text-left">
            {d.files.map((f) => <span key={f.path} className="rounded border border-line bg-raised px-1.5 py-px font-mono text-xs"><span className={f.status === "added" ? "text-accent" : f.status === "deleted" ? "text-danger" : "text-warn"}>{f.status[0].toUpperCase()}</span> {f.path}</span>)}
          </button>
        )}
      </div>
    );
  } else if (step.kind === "discover" && step.status === "completed") {
    const cmds = parse<CommandSpec[]>(step.output) ?? [];
    body = <div className="text-xs text-dim">{cmds.length === 0 ? "No test commands were found for this project." : `${cmds.length} command${cmds.length > 1 ? "s" : ""} found. Each one asks for approval below.`}</div>;
  } else if (step.kind === "command") {
    const spec = parse<CommandSpec>(step.detail);
    body = spec && (
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <code className="selectable flex min-w-0 flex-1 items-center gap-1.5 font-mono text-xs"><Terminal size={12} className="shrink-0 text-faint" /><span className="break-all">{spec.command}</span></code>
          <Badge tone={spec.source === "ai" ? "amber" : "neutral"}>{spec.source === "ai" ? "suggested by AI" : "from project files"}</Badge>
          {spec.risk === "high" && <Badge tone="red">destructive</Badge>}
        </div>
        {waiting && (
          <div className="flex items-center gap-3">
            <div className="flex-1 text-xs text-warn">
              Runs in the isolated workspace with credential-like environment variables removed. Network access is not blocked.
              {spec.risk === "high" && <div className="mt-1 flex items-center gap-1.5 font-medium text-danger"><AlertTriangle size={13} /> This command can delete data or use elevated privileges. Read it carefully.</div>}
            </div>
            <Button size="sm" onClick={() => decide(false)}>Skip</Button>
            <Button size="sm" variant={spec.risk === "high" ? "danger" : "primary"} onClick={() => decide(true)}>Approve & run</Button>
          </div>
        )}
        {output && <pre ref={pre} className="selectable max-h-64 overflow-auto rounded-md border border-line bg-bg px-3 py-2 font-mono text-xs leading-5 whitespace-pre-wrap text-dim">{output}</pre>}
      </div>
    );
  } else if (step.kind === "review" && step.status === "completed") {
    const r = parse<Review>(step.output);
    body = r && (
      <div className="space-y-2 text-[13px] leading-relaxed">
        <div className="flex items-center gap-2">
          <Badge tone={r.verdict === "approve" ? "green" : r.verdict === "no_changes" ? "neutral" : "amber"}>{r.verdict === "approve" ? "approves" : r.verdict === "no_changes" ? "nothing to review" : "changes requested"}</Badge>
        </div>
        <p className="selectable text-dim">{r.summary}</p>
        {r.issues.map((i, n) => (
          <div key={n} className="selectable flex gap-2 text-xs">
            <Badge tone={i.severity === "high" ? "red" : i.severity === "medium" ? "amber" : "neutral"}>{i.severity || "note"}</Badge>
            <span>{i.file && <span className="font-mono text-lav">{i.file} </span>}{i.note}</span>
          </div>
        ))}
      </div>
    );
  } else if (step.kind === "apply") {
    const failedTests = steps.filter((s) => s.kind === "command" && s.status === "failed").length;
    const review = parse<Review>(steps.find((s) => s.kind === "review")?.output ?? "");
    body = waiting ? (
      <div className="space-y-2">
        {(failedTests > 0 || (review && review.verdict !== "approve")) && (
          <div className="flex items-center gap-1.5 text-xs font-medium text-warn">
            <AlertTriangle size={13} />
            {failedTests > 0 && `${failedTests} test command${failedTests > 1 ? "s" : ""} failed. `}
            {review && review.verdict !== "approve" && "The Review agent requested changes."}
          </div>
        )}
        <div className="flex items-center gap-3">
          <div className="flex-1 text-xs text-warn">
            This writes the changes into <span className="font-medium">{run.project_name}</span>'s working tree. Nothing is staged or committed. If they collide with edits you made meanwhile, nothing is applied.
          </div>
          <Button size="sm" onClick={showChanges}><Eye size={12} /> View exact changes</Button>
          <Button size="sm" onClick={() => decide(false)}>Discard</Button>
          <Button size="sm" variant="primary" onClick={() => decide(true)}>Apply to project</Button>
        </div>
      </div>
    ) : step.output ? <div className="selectable text-xs text-dim">{step.output}</div> : null;
  }

  return (
    <Card className={clsx("overflow-hidden", waiting && "border-warn/40")} >
      <div className="flex items-center gap-3 px-4 py-2.5" data-step={step.kind} data-status={step.status}>
        <StepIcon status={step.status} />
        <RoleTag role={step.role} />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{step.title}</span>
        {step.attempts > 1 && <span className="text-[11px] text-warn">{step.attempts} attempts</span>}
        <span className="text-[11px] text-faint">{step.status.replace("_", " ")}</span>
      </div>
      {step.error && <div className="selectable border-t border-line px-4 py-2 text-xs text-danger">{step.error}</div>}
      {body && <div className={clsx("border-t px-4 py-3", waiting ? "border-warn/30 bg-warn/5" : "border-line")}>{body}</div>}
    </Card>
  );
}

// ------------------------------------------------------------------ run detail

function RunView({ id, ws, onDeleted, onChanged }: { id: string; ws: Workspace; onDeleted: () => void; onChanged: () => void }) {
  const [detail, setDetail] = useState<AgentRunDetail>();
  const [diff, setDiff] = useState<AgentDiff>();
  const [live, setLive] = useState<Record<string, string>>({});
  const [tab, setTab] = useState<"timeline" | "changes" | "logs">("timeline");
  const [role, setRole] = useState("");
  const [error, setError] = useState<string>();

  const load = () => api.getAgentRun(id).then(setDetail).catch((e) => setError(errorText(e)));
  const loadDiff = () => api.agentDiff(id).then(setDiff).catch(() => setDiff(undefined));
  useEffect(() => {
    void load();
    void loadDiff();
    const subs = [
      listen<string>(AGENT_CHANGED, (e) => {
        if (e.payload === id) {
          void load();
          void loadDiff();
          onChanged();
        }
      }),
      listen<{ run_id: string; step_id: string; chunk: string }>(AGENT_OUTPUT, (e) => {
        const { run_id, step_id, chunk } = e.payload;
        if (run_id === id) setLive((l) => ({ ...l, [step_id]: ((l[step_id] ?? "") + chunk).slice(-200_000) }));
      }),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const act = (fn: () => Promise<unknown>) => {
    setError(undefined);
    void fn().then(() => { void load(); onChanged(); }).catch((e) => setError(errorText(e)));
  };

  if (!detail) return error ? <div className="p-5"><ErrorBanner message={error} /></div> : <Spinner />;
  const { run, steps, events } = detail;
  const live_ = ["queued", "running", "paused"].includes(run.status);
  const shown = events.filter((e) => !role || e.role === role);

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <header className="shrink-0 border-b border-line px-5 py-3">
        <div className="flex items-center gap-2">
          <h1 className="min-w-0 flex-1 truncate text-[15px] font-semibold">{run.title}</h1>
          {run.pending_approvals > 0 && run.status === "running" && <Badge tone="amber">needs your approval</Badge>}
          <Badge tone={runTone[run.status]}>{run.status}</Badge>
          {run.status === "running" && <Button size="sm" onClick={() => act(() => api.agentPause(run.id))} title="Pause before the next step"><Pause size={12} /> Pause</Button>}
          {["paused", "failed", "interrupted"].includes(run.status) && <Button size="sm" variant="primary" onClick={() => act(() => api.agentResume(run.id))}><Play size={12} /> Resume</Button>}
          {(live_ || run.status === "interrupted" || run.status === "failed") && <Button size="sm" variant="danger" onClick={() => act(() => api.agentCancel(run.id))}><Square size={11} /> Cancel</Button>}
          {!live_ && (
            <Button
              size="sm"
              variant="ghost"
              title="Delete task and its history"
              onClick={async () => {
                if (await confirmAction(`Delete “${run.title}” and its history? Your project is not affected.`)) {
                  await api.deleteAgentRun(run.id).then(onDeleted).catch((e) => setError(errorText(e)));
                }
              }}
            >
              <Trash2 size={13} />
            </Button>
          )}
        </div>
        <div className="mt-1 text-xs text-dim">
          {run.project_name} · {run.provider_name ?? "provider removed"}{run.model ? ` · ${run.model}` : ""} · started {timeAgo(run.created_at)}
        </div>
        <p className="selectable mt-2 max-h-16 overflow-auto text-[13px] leading-relaxed whitespace-pre-wrap text-dim">{run.outcome}</p>
      </header>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[{ id: "timeline", label: "Timeline" }, { id: "changes", label: "Changes", count: diff?.files.length }, { id: "logs", label: "Logs", count: events.length }]}
      />
      <div className="empty:hidden p-3 pb-0"><ErrorBanner message={error ?? (run.status === "failed" || run.status === "interrupted" ? run.error : null)} /></div>

      {tab === "timeline" && (
        <div className="flex-1 space-y-2.5 overflow-auto p-5" data-testid="agent-timeline">
          {run.status === "queued" && <div className="text-xs text-dim">Queued. It starts when another task finishes (two run at a time).</div>}
          {steps.map((s) => (
            <StepCard key={s.id} step={s} steps={steps} run={run} live={live[s.id]} ws={ws} act={act} showChanges={() => setTab("changes")} />
          ))}
        </div>
      )}
      {tab === "changes" && (
        <div className="flex min-h-0 flex-1" data-testid="agent-changes">
          {!diff || diff.files.length === 0 ? (
            <Empty icon={<FileDiff size={26} />} title="No changes yet">The Developer's changes appear here as an exact diff once the plan is approved and implemented.</Empty>
          ) : (
            <>
              <div className="w-64 shrink-0 overflow-auto border-r border-line py-2">
                <div className="px-4 pb-1 text-xs font-semibold text-dim">{diff.files.length} file{diff.files.length > 1 ? "s" : ""}</div>
                {diff.files.map((f) => (
                  <div key={f.path} className="flex items-center gap-2 px-4 py-1 font-mono text-xs">
                    <span className={clsx("w-3 font-bold", f.status === "added" ? "text-accent" : f.status === "deleted" ? "text-danger" : "text-warn")}>{f.status[0].toUpperCase()}</span>
                    <span className="truncate">{f.path}</span>
                  </div>
                ))}
              </div>
              <div className="selectable min-w-0 flex-1 overflow-auto"><DiffView text={diff.patch} /></div>
            </>
          )}
        </div>
      )}
      {tab === "logs" && (
        <div className="flex min-h-0 flex-1 flex-col" data-testid="agent-logs">
          <div className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2">
            <Select aria-label="Agent" className="w-44!" value={role} onChange={(e) => setRole(e.target.value)}>
              <option value="">All agents</option>
              {["research", "architect", "developer", "testing", "review", "user", "engine"].map((r) => <option key={r} value={r}>{r === "user" ? "you" : r}</option>)}
            </Select>
            <span className="text-xs text-faint">{shown.length} entries</span>
          </div>
          <div className="flex-1 overflow-auto">
            <table className="w-full text-xs">
              <tbody className="divide-y divide-line/60">
                {shown.map((e) => (
                  <tr key={e.id}>
                    <td className="w-24 px-4 py-1.5 whitespace-nowrap text-faint tabular-nums">{new Date(e.at).toLocaleTimeString()}</td>
                    <td className="w-24 py-1.5"><RoleTag role={e.role} /></td>
                    <td className="w-20 py-1.5 text-faint">{e.level}</td>
                    <td className={clsx("selectable py-1.5 pr-4", e.level === "error" ? "text-danger" : e.level === "approval" ? "text-warn" : "text-dim")}>{e.message}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ screen

export default function AgentsView({ ws }: { ws: Workspace }) {
  const runs = useAsync(api.listAgentRuns, []);
  const roles = useAsync(api.agentRoles, []);
  const providers = useAsync(api.listProviders, []);
  const [selected, setSelected] = useState<string | "overview" | "new">("overview");

  useEffect(() => {
    const sub = listen(AGENT_CHANGED, () => void runs.reload());
    return () => void sub.then((off) => off());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const list = runs.data ?? [];
  const groups = useMemo(() => [
    ["Active", list.filter((r) => ACTIVE.includes(r.status))] as const,
    ["History", list.filter((r) => !ACTIVE.includes(r.status))] as const,
  ], [list]);
  const canCreate = (providers.data ?? []).some((p) => p.enabled) && ws.projects.length > 0;

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-64 shrink-0 flex-col border-r border-line">
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-line px-4">
          <span className="text-[13px] font-semibold">Agents</span>
          <Button size="sm" variant="primary" disabled={!canCreate} title={canCreate ? undefined : "Needs an AI provider and a project"} onClick={() => setSelected("new")}><Plus size={13} /> New</Button>
        </div>
        <div className="flex-1 overflow-auto p-2">
          <button onClick={() => setSelected("overview")} className={clsx("mb-1 flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-[13px] font-medium", selected === "overview" ? "bg-raised" : "text-dim hover:bg-raised/60")}>
            <Workflow size={14} /> Overview
          </button>
          {groups.map(([title, items]) => items.length > 0 && (
            <div key={title}>
              <div className="px-2.5 pt-3 pb-1 text-[11px] font-medium text-faint">{title}</div>
              {items.map((r) => (
                <button key={r.id} onClick={() => setSelected(r.id)} className={clsx("mb-0.5 block w-full rounded-md px-2.5 py-2 text-left", selected === r.id ? "bg-raised" : "hover:bg-raised/60")}>
                  <div className="truncate text-[13px] font-medium">{r.title}</div>
                  <div className="mt-0.5 flex items-center gap-2 text-xs text-faint">
                    {r.pending_approvals > 0 && r.status === "running" ? <Badge tone="amber">needs approval</Badge> : <Badge tone={runTone[r.status]}>{r.status}</Badge>}
                    <span className="ml-auto truncate">{r.project_name}</span>
                  </div>
                </button>
              ))}
            </div>
          ))}
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="empty:hidden p-3"><ErrorBanner message={runs.error ?? providers.error} /></div>
        {selected === "overview" ? (
          !roles.data || !providers.data ? <Spinner /> : <Overview roles={roles.data} runs={list} providers={providers.data} ws={ws} onNew={() => setSelected("new")} />
        ) : selected === "new" ? (
          <NewRun ws={ws} providers={providers.data ?? []} onCancel={() => setSelected("overview")} onCreated={(r) => { void runs.reload(); setSelected(r.id); }} />
        ) : (
          <RunView key={selected} id={selected} ws={ws} onChanged={() => void runs.reload()} onDeleted={() => { setSelected("overview"); void runs.reload(); }} />
        )}
      </div>
    </div>
  );
}
