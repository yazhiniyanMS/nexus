// Settings → Execution: how approved commands run. Every level is described as it is enforced on
// this computer; levels without a sandbox backend here are shown as unavailable, not hidden.
import clsx from "clsx";
import { useEffect, useState } from "react";
import { execSettingsChanged } from "../../components/ExecNote";
import { Button, ErrorBanner, Input, Spinner } from "../../components/ui";
import { api, type ExecCapabilities, type ExecLevel, type ExecSettings } from "../../lib/api";
import { errorText } from "../../lib/util";

const LEVELS: { id: ExecLevel; label: string }[] = [
  { id: "trusted", label: "Trusted" },
  { id: "restricted", label: "Restricted" },
  { id: "isolated", label: "Isolated" },
];

function LevelChoice({ name, value, caps, onChange }: { name: string; value: ExecLevel; caps: ExecCapabilities; onChange: (l: ExecLevel) => void }) {
  return (
    <div role="radiogroup" aria-label={name} className="space-y-1.5">
      {LEVELS.map((l) => {
        const info = caps.levels.find((x) => x.level === l.id);
        const available = info?.available ?? false;
        return (
          <label
            key={l.id}
            data-level={l.id}
            className={clsx(
              "flex cursor-pointer gap-2.5 rounded-md border px-3 py-2",
              value === l.id ? "border-accent/60 bg-accent/[0.05]" : "border-line",
              !available && "cursor-not-allowed opacity-60",
            )}
          >
            <input type="radio" name={name} className="mt-0.5" checked={value === l.id} disabled={!available} onChange={() => onChange(l.id)} />
            <span className="min-w-0">
              <span className="text-sm font-medium">{l.label}</span>
              {!available && <span className="ml-2 text-2xs text-warn">not available on this computer</span>}
              <span className="mt-0.5 block text-xs leading-5 text-dim">{info?.description}</span>
            </span>
          </label>
        );
      })}
    </div>
  );
}

function NumberField({ label, hint, value, min, max, onChange }: { label: string; hint?: string; value: number; min: number; max: number; onChange: (n: number) => void }) {
  return (
    <label className="grid grid-cols-[14rem_7rem_1fr] items-center gap-3 text-sm">
      <span className="text-dim">{label}</span>
      <Input type="number" min={min} max={max} value={Number.isFinite(value) ? value : ""} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="text-xs text-faint">{hint}</span>
    </label>
  );
}

export function Execution() {
  const [caps, setCaps] = useState<ExecCapabilities>();
  const [saved, setSaved] = useState<ExecSettings>();
  const [draft, setDraft] = useState<ExecSettings>();
  const [envText, setEnvText] = useState("");
  const [error, setError] = useState<string>();
  const [note, setNote] = useState<string>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    Promise.all([api.execCapabilities(), api.execSettings()])
      .then(([c, s]) => {
        setCaps(c);
        setSaved(s);
        setDraft(s);
        setEnvText(s.allowed_env.join("\n"));
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  if (!caps || !draft || !saved) return error ? <ErrorBanner message={error} /> : <Spinner />;
  const set = (patch: Partial<ExecSettings>) => {
    setNote(undefined);
    setDraft({ ...draft, ...patch });
  };
  const allowed = envText.split(/[\s,]+/).map((s) => s.trim()).filter(Boolean);
  const next: ExecSettings = { ...draft, allowed_env: allowed };
  const changed = JSON.stringify(next) !== JSON.stringify(saved);

  const save = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const s = await api.execSettingsSet(next);
      setSaved(s);
      setDraft(s);
      setEnvText(s.allowed_env.join("\n"));
      setNote("Saved. New commands use these settings; running ones keep theirs.");
      execSettingsChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-7" data-testid="exec-settings">
      <p className="text-sm leading-6 text-dim">
        Approval decides <em>whether</em> a command runs; these settings decide <em>what it can touch</em> when it does.{" "}
        {caps.backend ? (
          <>Isolation on this computer: <span className="text-ink">{caps.backend}</span>.</>
        ) : (
          <>No sandbox is available on this computer ({caps.os}), so commands can only run Trusted.</>
        )}
      </p>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Task steps</h3>
        <p className="text-xs text-faint">Commands you wrote or accepted from a plan, in Tasks.</p>
        <LevelChoice name="Level for task steps" value={draft.task_level} caps={caps} onChange={(l) => set({ task_level: l })} />
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Agent commands</h3>
        <p className="text-xs text-faint">Test commands an agent proposes or finds in the project's files. Each still needs your approval.</p>
        <LevelChoice name="Level for agent commands" value={draft.agent_level} caps={caps} onChange={(l) => set({ agent_level: l })} />
      </section>

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">Limits</h3>
        <NumberField label="Task step time limit (minutes)" hint="A task can set its own." value={draft.task_timeout_secs / 60} min={1} max={1440} onChange={(m) => set({ task_timeout_secs: Math.round(m * 60) })} />
        <NumberField label="Agent command time limit (minutes)" value={draft.agent_timeout_secs / 60} min={1} max={1440} onChange={(m) => set({ agent_timeout_secs: Math.round(m * 60) })} />
        <NumberField label="Output kept per command (KB)" hint="Past this, output is cut and a note says so." value={draft.output_limit_kb} min={16} max={10240} onChange={(n) => set({ output_limit_kb: n })} />
        <NumberField label="Largest file a command may write (MB)" hint="Restricted and Isolated only." value={draft.max_file_mb} min={1} max={1048576} onChange={(n) => set({ max_file_mb: n })} />
        <NumberField label="Memory per command (MB)" hint={caps.os === "linux" ? "Isolated only." : "Isolated on Linux only; macOS cannot enforce it."} value={draft.memory_mb} min={256} max={1048576} onChange={(n) => set({ memory_mb: n })} />
        <NumberField label="Agent tasks at once" value={draft.max_concurrent_agents} min={1} max={8} onChange={(n) => set({ max_concurrent_agents: n })} />
        <NumberField label="Agent tasks waiting at most" value={draft.max_queued_agents} min={1} max={200} onChange={(n) => set({ max_queued_agents: n })} />
        <NumberField label="Task runs at once" value={draft.max_running_tasks} min={1} max={32} onChange={(n) => set({ max_running_tasks: n })} />
        <p className="text-xs text-faint">When a limit passes, the command and everything it started are stopped, and its record says why.</p>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Environment variables you allow</h3>
        <p className="text-xs leading-5 text-faint">
          Trusted commands get your environment without anything that looks like a credential (keys, tokens, passwords, URLs with a
          password). Restricted and Isolated commands get only what tools need to find themselves (PATH, HOME, locale, toolchain
          folders). Name a variable here to pass it anyway, for example <code className="font-mono">GITHUB_TOKEN</code> for a task that
          really needs it. One per line.
        </p>
        <textarea
          aria-label="Allowed environment variables"
          className="h-24 w-full rounded-md border border-line bg-bg p-2 font-mono text-xs"
          value={envText}
          onChange={(e) => {
            setNote(undefined);
            setEnvText(e.target.value);
          }}
          spellCheck={false}
        />
      </section>

      <ErrorBanner message={error} />
      <div className="flex items-center gap-3">
        <Button variant="primary" disabled={!changed || busy} onClick={() => void save()}>Save</Button>
        {changed && <Button variant="ghost" onClick={() => { setDraft(saved); setEnvText(saved.allowed_env.join("\n")); }}>Discard changes</Button>}
        {note && <span role="status" className="text-xs text-ok">{note}</span>}
      </div>
    </div>
  );
}
