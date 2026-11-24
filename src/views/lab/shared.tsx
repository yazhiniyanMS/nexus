// Pieces shared by the Model Lab's tabs.
import clsx from "clsx";
import { Check, Copy, Globe, HardDrive } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Workspace } from "../../App";
import { Button, Input, Select } from "../../components/ui";
import type { GenParams, KindInfo, LabSettings, Library, Metrics, Provider } from "../../lib/api";
import { fmtInt, fmtMs, fmtTps, hostOf, PARAM_SPECS, parseParam, UNKNOWN, type ParamSpec } from "../../lib/lab";

export type LabTab = "library" | "runtimes" | "playground" | "benchmarks" | "compare" | "providers";

/** Where a tab should start when another tab sends the user to it. */
export interface LabTarget { providerId?: string; model?: string; suiteId?: string }

/** Data every tab needs, loaded once by the Model Lab screen. */
export interface LabCtx {
  ws: Workspace;
  providers: Provider[];
  kinds: KindInfo[];
  settings: LabSettings | undefined;
  library: Library | undefined;
  libraryBusy: boolean;
  libraryError: string | undefined;
  reloadProviders: () => Promise<void>;
  refreshLibrary: () => Promise<void>;
  reloadSettings: () => Promise<void>;
  go: (tab: LabTab, target?: LabTarget) => void;
}

export const kindOf = (ctx: LabCtx, provider: Provider | undefined) => ctx.kinds.find((k) => k.kind === provider?.kind);
export const usable = (providers: Provider[]) => providers.filter((p) => p.enabled);

/** Models the library has seen for a connection, favourites first. */
export function modelsOf(ctx: LabCtx, providerId: string): string[] {
  return (ctx.library?.models ?? []).filter((m) => m.provider_id === providerId && m.status !== "missing" && m.status !== "disabled").map((m) => m.model_id);
}

export function LocalBadge({ provider }: { provider: Provider }) {
  return provider.local ? (
    <span className="inline-flex items-center gap-1 text-2xs text-accent" title="Requests to this connection stay on this machine">
      <HardDrive size={11} /> on this machine
    </span>
  ) : (
    <span className="inline-flex items-center gap-1 text-2xs text-warn" title={`Requests to this connection go to ${hostOf(provider.base_url)}`}>
      <Globe size={11} /> remote · {hostOf(provider.base_url)}
    </span>
  );
}

/** Shown for a connection that is not on this machine: says where data goes and asks to confirm. */
export function RemoteNotice({ provider, what, confirmed, onConfirm }: { provider: Provider | undefined; what: string; confirmed: boolean; onConfirm: (v: boolean) => void }) {
  if (!provider || provider.local) return null;
  const host = hostOf(provider.base_url);
  return (
    <label data-testid="remote-notice" className="flex items-start gap-2 rounded-md border border-warn/30 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
      <input type="checkbox" className="mt-0.5 accent-warn" checked={confirmed} onChange={(e) => onConfirm(e.target.checked)} />
      <span>
        <span className="font-semibold">This leaves your machine.</span> {what} will be sent to <span className="font-mono">{host}</span> ({provider.name}). Tick to allow it.
      </span>
    </label>
  );
}

/** Pick a connection and one of its models. The model can also be typed, for runtimes that list none. */
export function ModelPicker({ ctx, providerId, model, onChange, disabled, idPrefix }: {
  ctx: LabCtx; providerId: string; model: string; onChange: (providerId: string, model: string) => void; disabled?: boolean; idPrefix: string;
}) {
  const options = usable(ctx.providers);
  const known = modelsOf(ctx, providerId);
  return (
    <div className="flex min-w-0 items-center gap-2">
      <Select
        aria-label="Connection"
        className="w-48!"
        value={providerId}
        disabled={disabled}
        onChange={(e) => {
          const p = options.find((x) => x.id === e.target.value);
          const first = modelsOf(ctx, e.target.value)[0];
          onChange(e.target.value, p?.default_model || first || "");
        }}
      >
        {!options.some((p) => p.id === providerId) && <option value="">Choose a connection…</option>}
        {options.map((p) => <option key={p.id} value={p.id}>{p.name}{p.local ? "" : " (remote)"}</option>)}
      </Select>
      <Input
        aria-label="Model"
        className="w-64! font-mono"
        list={`${idPrefix}-models`}
        placeholder={known.length ? "Choose or type a model" : "Model name"}
        value={model}
        disabled={disabled || !providerId}
        onChange={(e) => onChange(providerId, e.target.value)}
      />
      <datalist id={`${idPrefix}-models`}>{known.map((m) => <option key={m} value={m} />)}</datalist>
    </div>
  );
}

/** Inputs for the generation settings a kind of runtime accepts. Empty means "not set". */
export function ParamsEditor({ kind, value, onChange, onError, disabled }: {
  kind: KindInfo | undefined; value: GenParams; onChange: (p: GenParams) => void; onError?: (message: string | undefined) => void; disabled?: boolean;
}) {
  const supported = kind?.params ?? [];
  const specs = PARAM_SPECS.filter((s) => supported.includes(s.key));
  // Text is kept separately so a half-typed number does not get rewritten under the cursor.
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const signature = specs.map((s) => `${s.key}=${value[s.key] ?? ""}`).join("&");
  useEffect(() => {
    setTexts((old) => {
      const next: Record<string, string> = {};
      for (const s of specs) {
        const current = value[s.key];
        const typed = old[s.key];
        // Keep what the user typed while it still means the same number.
        next[s.key] = typed !== undefined && parseParam(s, typed).value === current && !parseParam(s, typed).error ? typed : current === null ? "" : String(current);
      }
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  const edit = (spec: ParamSpec, text: string) => {
    setTexts((t) => ({ ...t, [spec.key]: text }));
    const parsed = parseParam(spec, text);
    const nextErrors = { ...errors };
    if (parsed.error) nextErrors[spec.key] = parsed.error;
    else {
      delete nextErrors[spec.key];
      onChange({ ...value, [spec.key]: parsed.value });
    }
    setErrors(nextErrors);
    onError?.(Object.values(nextErrors)[0]);
  };

  if (!kind) return null;
  return (
    <div className="space-y-2">
      <div className="grid grid-cols-2 gap-2">
        {specs.map((s) => (
          <label key={s.key} className="block" title={s.hint}>
            <div className="mb-1 text-2xs font-medium text-dim">{s.label}</div>
            <Input
              aria-label={s.label}
              inputMode="decimal"
              className={clsx("font-mono", errors[s.key] && "border-danger/60!")}
              placeholder="runtime default"
              value={texts[s.key] ?? ""}
              disabled={disabled}
              onChange={(e) => edit(s, e.target.value)}
            />
          </label>
        ))}
        {supported.includes("think") && (
          <label className="block" title="Thinking-capable models can answer without thinking first">
            <div className="mb-1 text-2xs font-medium text-dim">Thinking</div>
            <Select
              aria-label="Thinking"
              value={value.think === null ? "" : value.think ? "on" : "off"}
              disabled={disabled}
              onChange={(e) => onChange({ ...value, think: e.target.value === "" ? null : e.target.value === "on" })}
            >
              <option value="">model default</option>
              <option value="off">off</option>
              <option value="on">on</option>
            </Select>
          </label>
        )}
      </div>
      {Object.values(errors)[0] && <div className="text-xs text-danger">{Object.values(errors)[0]}</div>}
      {kind.kind === "anthropic" && <div className="text-2xs leading-relaxed text-faint">Current Claude models reject temperature, top-p and top-k, so NEXUS does not send them.</div>}
    </div>
  );
}

function Metric({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <span className="whitespace-nowrap" title={value === UNKNOWN ? "Not reported by this runtime" : title}>
      <span className="text-faint">{label}</span> <span className={clsx("tabular-nums", value === UNKNOWN ? "text-faint" : "text-dim")}>{value}</span>
    </span>
  );
}

/** The measurements of one request. Unknown values are shown as unknown. */
export function MetricsLine({ metrics, className }: { metrics: Metrics; className?: string }) {
  const derived = metrics.tps_source === "derived";
  return (
    <div data-testid="metrics" className={clsx("flex flex-wrap gap-x-4 gap-y-1 text-2xs", className)}>
      <Metric label="first token" value={fmtMs(metrics.ttft_ms)} title="Measured by NEXUS: request sent to first generated text" />
      <Metric label="total" value={fmtMs(metrics.total_ms)} title="Measured by NEXUS: request sent to end of the response" />
      <Metric label="in" value={fmtInt(metrics.input_tokens)} title="Input tokens, as reported by the runtime" />
      <Metric label="out" value={fmtInt(metrics.output_tokens)} title="Output tokens, as reported by the runtime" />
      <Metric
        label="speed"
        value={metrics.tokens_per_second === null ? UNKNOWN : `${fmtTps(metrics.tokens_per_second)}${derived ? " (derived)" : ""}`}
        title={derived ? "Derived by NEXUS: reported output tokens divided by the time after the first token" : "Reported by the runtime from its own generation time"}
      />
      {metrics.load_ms !== null && <Metric label="load" value={fmtMs(metrics.load_ms)} title="Time the runtime spent loading the model for this request" />}
      {metrics.prompt_eval_ms !== null && <Metric label="prompt" value={fmtMs(metrics.prompt_eval_ms)} title="Time the runtime spent processing the prompt" />}
      {metrics.stop_reason && metrics.stop_reason !== "stop" && metrics.stop_reason !== "end_turn" && <Metric label="stopped" value={metrics.stop_reason} />}
    </div>
  );
}

/** Copy text to the clipboard, with a visible confirmation. */
export function CopyButton({ text, label = "Copy", size = "sm" }: { text: string; label?: string; size?: "sm" | "md" }) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Older webviews: fall back to a temporary selection.
      const area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      document.execCommand("copy");
      area.remove();
    }
    setDone(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setDone(false), 1500);
  };
  return (
    <Button size={size} variant="ghost" type="button" onClick={() => void copy()} disabled={!text}>
      {done ? <Check size={12} className="text-accent" /> : <Copy size={12} />} {done ? "Copied" : label}
    </Button>
  );
}

export { ConfirmButton } from "../../components/ui";

export function Section({ title, hint, actions, children }: { title: string; hint?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <div className="flex items-end gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">{title}</h2>
          {hint && <div className="text-xs leading-relaxed text-dim">{hint}</div>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

/** A thin progress bar; indeterminate (no fill) when the total is not known. */
export function Progress({ value }: { value: number | null }) {
  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-raised" role="progressbar" aria-valuenow={value ?? undefined} aria-valuemin={0} aria-valuemax={100}>
      <div className={clsx("h-full bg-accent", value === null && "w-1/4 animate-pulse")} style={value === null ? undefined : { width: `${value}%` }} />
    </div>
  );
}
