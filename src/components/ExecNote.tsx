// One line under an approval saying exactly how the command will run: its level as enforced on
// this machine, and its time limit. Trusted is never described as a sandbox.
import { ShieldCheck, ShieldOff } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type ExecCapabilities, type ExecSettings } from "../lib/api";

type Loaded = { settings: ExecSettings; caps: ExecCapabilities };
let cache: Promise<Loaded> | null = null;
const listeners = new Set<() => void>();

function load(): Promise<Loaded> {
  cache ??= Promise.all([api.execSettings(), api.execCapabilities()]).then(([settings, caps]) => ({ settings, caps }));
  return cache;
}

/** Call after the execution settings change so every note shows the new values. */
export function execSettingsChanged() {
  cache = null;
  listeners.forEach((l) => l());
}

export function formatLimit(secs: number): string {
  if (secs % 3600 === 0) return `${secs / 3600} h`;
  if (secs >= 60 && secs % 60 === 0) return `${secs / 60} min`;
  if (secs >= 60) return `${Math.floor(secs / 60)} min ${secs % 60} s`;
  return `${secs} s`;
}

const LABEL = { trusted: "Trusted", restricted: "Restricted", isolated: "Isolated" } as const;

export function ExecNote({ kind, timeoutSecs }: { kind: "task" | "agent"; timeoutSecs?: number | null }) {
  const [data, setData] = useState<Loaded>();
  useEffect(() => {
    let live = true;
    const refresh = () => void load().then((d) => live && setData(d)).catch(() => undefined);
    refresh();
    listeners.add(refresh);
    return () => {
      live = false;
      listeners.delete(refresh);
    };
  }, []);
  if (!data) return null;
  const level = kind === "task" ? data.settings.task_level : data.settings.agent_level;
  const limit = timeoutSecs ?? (kind === "task" ? data.settings.task_timeout_secs : data.settings.agent_timeout_secs);
  const info = data.caps.levels.find((l) => l.level === level);
  const sandboxed = level !== "trusted";
  const Icon = sandboxed ? ShieldCheck : ShieldOff;
  return (
    <div data-testid="exec-note" title={info?.description} className="flex items-center gap-1.5 text-2xs text-faint">
      <Icon size={12} className={sandboxed ? "text-ok" : "text-warn"} aria-hidden />
      <span>
        Runs <span className="text-dim">{LABEL[level]}</span>
        {sandboxed ? ` (${data.caps.backend ?? "sandbox"})` : " — as you, not sandboxed"} · time limit {formatLimit(limit)}
      </span>
    </div>
  );
}
