import clsx from "clsx";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Check, Eye, FileDiff, Minus, Pause, Play, Plus, Square, Terminal, Trash2, Workflow } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Workspace } from "../App";
import { ModelChoiceControls, ModelChoiceNotice, useModelChoice } from "../components/ModelChoice";
import { Badge, Button, ConfirmButton, DiffView, Empty, ErrorBanner, Field, Select, Spinner, Status, Tabs, ViewHeader, type StatusKind } from "../components/ui";
import { ExecNote } from "../components/ExecNote";
import { AGENT_CHANGED, AGENT_OUTPUT, api, type AgentDiff, type AgentRun, type AgentRunDetail, type AgentStep, type AgentStepStatus, type Provider, type RoleInfo } from "../lib/api";
import { errorText, timeAgo, useAsync } from "../lib/util";

/** Run states as the shared Status component shows them. "Needs you" wins over the run's own state. */
export function runKind(r: AgentRun): StatusKind {
  if (r.pending_approvals > 0 && r.status === "running") return "waiting";
  switch (r.status) {
    case "queued": return "queued";
    case "running": return "running";
    case "paused": case "interrupted": return "waiting";
    case "completed": return "done";
    case "failed": return "failed";
    default: return "cancelled";
  }
}
const runLabel = (r: AgentRun) =>
  r.pending_approvals > 0 && r.status === "running" ? "Needs approval" : r.status === "interrupted" ? "Interrupted" : r.status === "paused" ? "Paused" : r.status === "rejected" ? "Rejected" : r.status === "discarded" ? "Discarded" : undefined;

const stepKind: Record<AgentStepStatus, StatusKind> = {
  pending: "idle", running: "running", awaiting_approval: "waiting", completed: "done", failed: "failed", skipped: "cancelled", rejected: "cancelled", cancelled: "cancelled",
};
const stepLabel: Partial<Record<AgentStepStatus, string>> = { awaiting_approval: "Needs approval", skipped: "Skipped", rejected: "Rejected", pending: "Pending" };

/** How long a run has taken, or has been going. */
export function runDuration(r: { created_at: string; finished_at: string | null }, now = Date.now()): string {
  const ms = (r.finished_at ? new Date(r.finished_at).getTime() : now) - new Date(r.created_at).getTime();
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  return h < 48 ? `${h}h ${String(m % 60).padStart(2, "0")}m` : `${Math.floor(h / 24)}d`;
}

function RoleTag({ role }: { role: string }) {
  return <span className="inline-flex w-20 shrink-0 text-xs font-medium text-dim capitalize">{role === "user" ? "you" : role}</span>;
}

function parse<T>(text: string): T | null {
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}


// ------------------------------------------------------------------ overview

const BOARD: { title: string; match: (r: AgentRun) => boolean; hint: string }[] = [
  { title: "Needs your approval", match: (r) => r.status === "running" && r.pending_approvals > 0, hint: "A plan, a command or the final diff is waiting for you." },
  { title: "Running", match: (r) => r.status === "running" && r.pending_approvals === 0, hint: "" },
  { title: "Queued", match: (r) => r.status === "queued", hint: "Two tasks run at a time." },
  { title: "Paused or interrupted", match: (r) => r.status === "paused" || r.status === "interrupted", hint: "Resume to continue from the last finished step." },
  { title: "Failed", match: (r) => r.status === "failed", hint: "Open one to see the error; most can be resumed." },
  { title: "Finished", match: (r) => ["completed", "cancelled", "rejected", "discarded"].includes(r.status), hint: "" },
];

const CAPS: { key: keyof Pick<RoleInfo, "can_read" | "can_write_workspace" | "can_run_commands">; label: string }[] = [
  { key: "can_read", label: "Read project" },
  { key: "can_write_workspace", label: "Write isolated copy" },
  { key: "can_run_commands", label: "Run approved commands" },
];

function Overview({ roles, runs, providers: allProviders, ws, onNew, onOpen }: {
  roles: RoleInfo[]; runs: AgentRun[]; providers: Provider[]; ws: Workspace; onNew: () => void; onOpen: (id: string) => void;
}) {
  // Only connections that are switched on can run agents.
  const providers = allProviders.filter((p) => p.enabled);
  const [now, setNow] = useState(Date.now());
  const anyLive = runs.some((r) => ["running", "queued"].includes(r.status));
  useEffect(() => {
    if (!anyLive) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [anyLive]);

  return (
    <div className="flex-1 overflow-auto">
      <div className="mx-auto max-w-5xl space-y-6 px-6 py-5">
        {providers.length === 0 && (
          <div className="flex items-start gap-3 rounded-md border border-warn/30 bg-warn/5 p-3" data-testid="agent-setup">
            <AlertTriangle size={15} className="mt-0.5 shrink-0 text-warn" />
            <div className="flex-1 text-sm leading-relaxed">
              <div className="font-medium">Agents need a model</div>
              <div className="text-dim">
                {allProviders.length === 0 ? "No model connection is configured" : "Every model connection is disabled"}. Model Lab finds local runtimes such as Ollama on this machine; other connections are added in Settings. The rest of NEXUS works without one.
              </div>
            </div>
            <Button size="sm" onClick={() => ws.go("models")}>Open Model Lab</Button>
          </div>
        )}
        {providers.length > 0 && ws.projects.length === 0 && <p className="text-sm text-dim">Import a project first. Agents work on a Git repository with at least one commit.</p>}

        {runs.length === 0 ? (
          <div className="py-10 text-center">
            <Workflow size={26} className="mx-auto text-faint" />
            <div className="mt-2 text-md font-semibold">No agent tasks yet</div>
            <p className="mx-auto mt-1 max-w-md text-sm text-dim">
              Describe an outcome. Agents research, plan, implement, test and review it in an isolated copy of the repository. You approve the plan, every command and the final diff.
            </p>
            <Button className="mt-4" variant="primary" onClick={onNew} disabled={providers.length === 0 || ws.projects.length === 0}><Plus size={14} /> New agent task</Button>
          </div>
        ) : (
          <div className="space-y-5" data-testid="agent-board">
            {BOARD.map((g) => {
              const items = runs.filter(g.match);
              if (items.length === 0) return null;
              return (
                <section key={g.title} aria-label={g.title}>
                  <div className="mb-1 flex items-baseline gap-2">
                    <h2 className={clsx("text-xs font-semibold", g.title === "Needs your approval" ? "text-warn" : g.title === "Failed" ? "text-danger" : "text-dim")}>{g.title}</h2>
                    <span className="text-2xs text-faint">{items.length}</span>
                    {g.hint && <span className="text-2xs text-faint">· {g.hint}</span>}
                  </div>
                  <table className="w-full text-sm">
                    <tbody className="divide-y divide-line/60 border-y border-line/60">
                      {items.map((r) => (
                        <tr key={r.id} data-run={r.status} className="cursor-default hover:bg-raised/50" onClick={() => onOpen(r.id)}>
                          <td className="w-36 py-2 pl-1"><Status kind={runKind(r)} label={runLabel(r)} /></td>
                          <td className="max-w-0 py-2 pr-3"><div className="truncate">{r.title}</div>{r.error && r.status === "failed" && <div className="truncate text-2xs text-danger">{r.error}</div>}</td>
                          <td className="w-40 truncate py-2 pr-3 text-xs text-dim">{r.project_name ?? "project removed"}</td>
                          <td className="w-44 truncate py-2 pr-3 font-mono text-2xs text-faint">{r.model ?? "—"}</td>
                          <td className="w-20 py-2 text-right text-xs text-dim tabular-nums" title="Duration">{runDuration(r, now)}</td>
                          <td className="w-24 py-2 pr-1 text-right text-2xs text-faint">{timeAgo(r.updated_at)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </section>
              );
            })}
          </div>
        )}

        <section aria-label="Agent permissions">
          <div className="mb-1 flex items-baseline gap-2">
            <h2 className="text-xs font-semibold text-dim">What each agent may do</h2>
            <span className="text-2xs text-faint">· no agent can write to your project; only your approval of the final diff can</span>
          </div>
          <table className="w-full text-xs" data-testid="agent-roles">
            <thead className="text-left text-faint">
              <tr className="border-b border-line"><th className="py-1.5 pl-1 font-medium">Agent</th><th className="font-medium">Does</th>{CAPS.map((c) => <th key={c.key} className="w-36 text-center font-medium">{c.label}</th>)}</tr>
            </thead>
            <tbody className="divide-y divide-line/60">
              {roles.map((r) => (
                <tr key={r.id}>
                  <td className="w-24 py-1.5 pl-1 font-medium capitalize">{r.name}</td>
                  <td className="py-1.5 pr-3 text-dim">{r.responsibilities.join(" · ")}</td>
                  {CAPS.map((c) => (
                    <td key={c.key} className="text-center">
                      {r[c.key] ? <Check size={13} className="inline text-ok" aria-label="allowed" /> : <Minus size={13} className="inline text-faint" aria-label="not allowed" />}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </section>
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
        <h2 className="text-md font-semibold">New agent task</h2>
        <ErrorBanner message={error} />
        <Field label="Desired outcome" hint="Describe what should be true when the task is done. The Architect turns this into a plan you approve before anything is written.">
          <textarea
            autoFocus
            aria-label="Desired outcome"
            value={outcome}
            onChange={(e) => setOutcome(e.target.value)}
            rows={5}
            placeholder="e.g. Add a --json flag to the export command that prints the report as JSON, with a test."
            className="w-full rounded-md border border-line bg-bg p-2.5 text-sm placeholder:text-faint focus:border-lav/60 focus:outline-none"
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
    <div className="selectable space-y-3 text-sm leading-relaxed">
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
      <div className="space-y-2 text-sm leading-relaxed">
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
      <div className="space-y-2 text-sm">
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
            <div className="flex-1 space-y-1 text-xs text-warn">
              <div>Runs in this task's own copy of the project, not in your project folder.</div>
              <ExecNote kind="agent" />
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
      <div className="space-y-2 text-sm leading-relaxed">
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
    <div className={clsx("border-b border-line/60", waiting && "border-l-2 border-l-warn bg-warn/[0.04]")}>
      <div className="flex items-center gap-3 px-4 py-2.5" data-step={step.kind} data-status={step.status}>
        <Status kind={stepKind[step.status]} label={stepLabel[step.status]} showLabel={false} />
        <RoleTag role={step.role} />
        <span className="min-w-0 flex-1 truncate text-sm font-medium">{step.title}</span>
        {step.attempts > 1 && <span className="text-2xs text-warn">{step.attempts} attempts</span>}
        <span className={clsx("text-2xs", step.status === "awaiting_approval" ? "font-medium text-warn" : "text-faint")}>{stepLabel[step.status] ?? step.status.replace("_", " ")}</span>
      </div>
      {step.error && <div className="selectable px-4 pb-2.5 pl-[7.75rem] text-xs text-danger">{step.error}</div>}
      {body && <div className="px-4 pb-3 pl-[7.75rem]">{body}</div>}
    </div>
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
          <h1 className="min-w-0 flex-1 truncate text-md font-semibold">{run.title}</h1>
          <Status kind={runKind(run)} label={runLabel(run)} />
          {run.status === "running" && <Button size="sm" onClick={() => act(() => api.agentPause(run.id))} title="Pause before the next step"><Pause size={12} /> Pause</Button>}
          {["paused", "failed", "interrupted"].includes(run.status) && <Button size="sm" variant="primary" onClick={() => act(() => api.agentResume(run.id))}><Play size={12} /> Resume</Button>}
          {(live_ || run.status === "interrupted" || run.status === "failed") && <Button size="sm" variant="danger" onClick={() => act(() => api.agentCancel(run.id))}><Square size={11} /> Cancel</Button>}
          {!live_ && (
            <ConfirmButton
              label=""
              icon={<Trash2 size={13} />}
              title="Delete task and its history"
              confirm="Delete task — project not affected"
              onConfirm={() => api.deleteAgentRun(run.id).then(onDeleted).catch((e) => setError(errorText(e)))}
            />
          )}
        </div>
        <div className="mt-1 flex flex-wrap gap-x-3 text-xs text-dim">
          <span>{run.project_name ?? "project removed"}</span>
          <span className="font-mono">{run.provider_name ?? "connection removed"}{run.model ? ` · ${run.model}` : ""}</span>
          <span>started {timeAgo(run.created_at)}</span>
          <span className="tabular-nums">{run.finished_at ? `took ${runDuration(run)}` : `running for ${runDuration(run)}`}</span>
          <span>{steps.filter((x) => x.status === "completed").length} of {steps.length} steps done</span>
        </div>
        {run.outcome.trim() !== run.title.trim() && <p className="selectable mt-2 max-h-16 overflow-auto text-sm leading-relaxed whitespace-pre-wrap text-dim">{run.outcome}</p>}
      </header>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[{ id: "timeline", label: "Timeline" }, { id: "changes", label: "Changes", count: diff?.files.length }, { id: "logs", label: "Logs", count: events.length }]}
      />
      <div className="empty:hidden p-3 pb-0"><ErrorBanner message={error ?? ((run.status === "failed" || run.status === "interrupted") && !steps.some((x) => x.error && x.error === run.error) ? run.error : null)} /></div>

      {tab === "timeline" && (
        <div className="flex-1 overflow-auto pb-5" data-testid="agent-timeline">
          {run.status === "queued" && <div className="px-4 py-2 text-xs text-dim">Queued. It starts when another task finishes (two run at a time).</div>}
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
  const groups = useMemo(() => BOARD.map((g) => [g.title, list.filter(g.match)] as const), [list]);
  const canCreate = (providers.data ?? []).some((p) => p.enabled) && ws.projects.length > 0;

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-64 shrink-0 flex-col border-r border-line">
        <ViewHeader
          title="Agents"
          actions={<Button size="sm" variant="primary" disabled={!canCreate} title={canCreate ? "New agent task" : "Needs a model connection and a project"} onClick={() => setSelected("new")}><Plus size={13} /> New</Button>}
        />
        <div className="flex-1 overflow-auto p-2">
          <button onClick={() => setSelected("overview")} aria-current={selected === "overview" ? "page" : undefined} className={clsx("mb-1 flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-sm font-medium", selected === "overview" ? "bg-raised text-ink" : "text-dim hover:bg-raised/60")}>
            <Workflow size={14} /> All tasks
            <span className="ml-auto text-2xs text-faint">{list.length || ""}</span>
          </button>
          {groups.map(([title, items]) => items.length > 0 && (
            <div key={title}>
              <div className="px-2.5 pt-3 pb-1 text-2xs font-medium text-faint">{title}</div>
              {items.map((r) => (
                <button key={r.id} onClick={() => setSelected(r.id)} className={clsx("mb-0.5 flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left", selected === r.id ? "bg-raised" : "hover:bg-raised/60")}>
                  <Status kind={runKind(r)} label={runLabel(r)} showLabel={false} className="mt-[3px]" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm">{r.title}</span>
                    <span className="block truncate text-2xs text-faint">{r.project_name} · {timeAgo(r.updated_at)}</span>
                  </span>
                </button>
              ))}
            </div>
          ))}
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="empty:hidden p-3"><ErrorBanner message={runs.error ?? providers.error} /></div>
        {selected === "overview" ? (
          !roles.data || !providers.data ? <Spinner /> : <Overview roles={roles.data} runs={list} providers={providers.data} ws={ws} onNew={() => setSelected("new")} onOpen={setSelected} />
        ) : selected === "new" ? (
          <NewRun ws={ws} providers={providers.data ?? []} onCancel={() => setSelected("overview")} onCreated={(r) => { void runs.reload(); setSelected(r.id); }} />
        ) : (
          <RunView key={selected} id={selected} ws={ws} onChanged={() => void runs.reload()} onDeleted={() => { setSelected("overview"); void runs.reload(); }} />
        )}
      </div>
    </div>
  );
}
