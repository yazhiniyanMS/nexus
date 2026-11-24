// A read-only look at the local model runtimes, for screens outside the Model Lab (the Terminal).
// It only shows status and addresses to copy; it never types or runs anything.
import clsx from "clsx";
import { Check, Copy, FlaskConical } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type RuntimeReport } from "../lib/api";
import { errorText } from "../lib/util";

function CopyText({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      title="Copy"
      aria-label={`Copy ${text}`}
      className="grid size-5 shrink-0 place-items-center rounded text-faint hover:bg-raised hover:text-ink"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setDone(true);
          setTimeout(() => setDone(false), 1200);
        }).catch(() => undefined);
      }}
    >
      {done ? <Check size={11} className="text-accent" /> : <Copy size={11} />}
    </button>
  );
}

export function RuntimeChip({ onOpenLab }: { onOpenLab: () => void }) {
  const [open, setOpen] = useState(false);
  const [report, setReport] = useState<RuntimeReport>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  const load = async () => {
    setBusy(true);
    try {
      setReport(await api.labRuntimes());
      setError(undefined);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void load();
  }, []);
  useEffect(() => {
    if (!open) return;
    void load();
    const close = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const running = (report?.runtimes ?? []).flatMap((r) => r.endpoints.filter((e) => e.running).map((e) => ({ runtime: r, endpoint: e })));
  const label = !report ? "Runtimes…" : running.length === 0 ? "No model runtime running" : `${running[0].runtime.name.split(" (")[0]} running${running.length > 1 ? ` +${running.length - 1}` : ""}`;

  return (
    <div ref={box} className="relative">
      <button
        type="button"
        data-testid="runtime-chip"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        title="Local model runtimes"
        className="flex h-8 items-center gap-1.5 rounded-md border border-line px-2.5 text-xs text-dim hover:text-ink"
      >
        <span className={clsx("size-1.5 rounded-full", running.length ? "bg-accent" : "bg-faint")} />
        {label}
      </button>
      {open && (
        <div data-testid="runtime-popover" className="absolute right-0 top-9 z-30 w-[26rem] space-y-2 rounded-md border border-line bg-panel p-3 shadow-xl">
          <div className="flex items-center gap-2">
            <span className="text-xs font-semibold">Model runtimes on this machine</span>
            <button type="button" className="ml-auto text-2xs text-faint hover:text-ink" onClick={() => void load()}>{busy ? "checking…" : "refresh"}</button>
          </div>
          {error && <div className="text-xs text-danger">{error}</div>}
          {report?.runtimes.map((r) => (
            <div key={r.id} className="rounded-md border border-line/70 p-2">
              <div className="flex items-center gap-2 text-xs">
                <span className="font-medium">{r.name}</span>
                <span className="text-faint">{r.installed ? `installed${r.installed.version ? ` ${r.installed.version}` : ""}` : "not installed"}</span>
              </div>
              {r.installed && (
                <div className="mt-0.5 flex items-center gap-1">
                  <span className="selectable min-w-0 flex-1 truncate font-mono text-2xs text-faint">{r.installed.path}</span>
                  <CopyText text={r.installed.path} />
                </div>
              )}
              {r.endpoints.map((e) => (
                <div key={e.url} className="mt-1 flex items-center gap-1.5 text-2xs">
                  <span className={clsx("size-1.5 shrink-0 rounded-full", e.running ? "bg-accent" : "bg-faint")} />
                  <span className="selectable font-mono text-dim">{e.url}</span>
                  <span className="text-faint">
                    {e.running ? `answering${e.version ? ` · ${e.version}` : ""}${e.models !== null ? ` · ${e.models} model${e.models === 1 ? "" : "s"}` : ""}` : "not answering"}
                  </span>
                  <span className="ml-auto"><CopyText text={e.url} /></span>
                </div>
              ))}
            </div>
          ))}
          {report?.managed.filter((m) => m.status !== "exited").map((m) => (
            <div key={m.id} className="text-2xs text-dim">Started by NEXUS: <span className="font-mono">{m.url}</span> ({m.status})</div>
          ))}
          <div className="flex items-center gap-2 border-t border-line/60 pt-2">
            <span className="min-w-0 flex-1 text-2xs leading-relaxed text-faint">Status only. NEXUS never types into your terminal; copy an address if you need it in a command.</span>
            <button type="button" className="flex shrink-0 items-center gap-1 text-xs text-accent hover:underline" onClick={() => { setOpen(false); onOpenLab(); }}>
              <FlaskConical size={12} /> Model Lab
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
