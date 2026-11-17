// Pure helpers for the Model Lab: formatting measurements, filtering and sorting the model
// library, and handling generation settings. No imports, so they can be tested without a browser.
//
// The rule throughout: a value nobody reported is shown as unknown, never as zero.

export const UNKNOWN = "—";

type Maybe = number | null | undefined;
const known = (v: Maybe): v is number => typeof v === "number" && Number.isFinite(v);

/** A duration measured in milliseconds, in the unit a person would read it in. */
export function fmtMs(ms: Maybe): string {
  if (!known(ms)) return UNKNOWN;
  if (ms < 1) return "<1 ms";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  const minutes = Math.floor(ms / 60_000);
  return `${minutes}m ${String(Math.round((ms - minutes * 60_000) / 1000)).padStart(2, "0")}s`;
}

export function fmtTps(v: Maybe): string {
  if (!known(v)) return UNKNOWN;
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} tok/s`;
}

export function fmtInt(v: Maybe): string {
  return known(v) ? Math.round(v).toLocaleString("en-US") : UNKNOWN;
}

export function fmtBytes(n: Maybe): string {
  if (!known(n)) return UNKNOWN;
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

/** Parameter count: the runtime's own wording when it gave one, else the count rounded. */
export function fmtParams(count: Maybe, label?: string | null): string {
  if (label && label.trim()) return label.trim();
  if (!known(count) || count <= 0) return UNKNOWN;
  if (count >= 1e9) return `${(count / 1e9).toFixed(count >= 1e10 ? 0 : 1)}B`;
  if (count >= 1e6) return `${(count / 1e6).toFixed(count >= 1e8 ? 0 : 1)}M`;
  if (count >= 1e3) return `${(count / 1e3).toFixed(0)}K`;
  return String(count);
}

export function fmtTokens(n: Maybe): string {
  return known(n) ? `${fmtInt(n)} tokens` : UNKNOWN;
}

/** Progress as a whole percentage, or null when the total is not known. */
export function percent(received: Maybe, total: Maybe): number | null {
  if (!known(received) || !known(total) || total <= 0) return null;
  return Math.max(0, Math.min(100, Math.floor((received / total) * 100)));
}

/** Host (and port) of a URL, for saying where a request goes. */
export function hostOf(url: string): string {
  const m = /^[a-z][a-z0-9+.-]*:\/\/([^/?#]+)/i.exec(url.trim());
  return m ? m[1].replace(/^[^@]*@/, "") : url;
}

// ---------------------------------------------------------------- generation settings

export interface Params {
  temperature: number | null; top_p: number | null; top_k: number | null; max_tokens: number | null;
  seed: number | null; repeat_penalty: number | null; think: boolean | null;
}
export type ParamKey = keyof Params;

export const emptyParams = (): Params => ({ temperature: null, top_p: null, top_k: null, max_tokens: null, seed: null, repeat_penalty: null, think: null });

/** Fill in fields missing from a stored value (older saves, or the backend's defaults). */
export function normalizeParams(p: Partial<Params> | null | undefined): Params {
  return { ...emptyParams(), ...Object.fromEntries(Object.entries(p ?? {}).filter(([, v]) => v !== undefined)) } as Params;
}

/** Keep only the settings a runtime accepts; the rest are cleared rather than silently sent. */
export function paramsFor(p: Params, supported: readonly string[]): Params {
  const out = emptyParams();
  for (const key of Object.keys(out) as ParamKey[]) {
    if (supported.includes(key)) (out as unknown as Record<string, unknown>)[key] = p[key];
  }
  return out;
}

export interface ParamSpec { key: Exclude<ParamKey, "think">; label: string; min: number; max: number; step: number; integer: boolean; hint: string }

export const PARAM_SPECS: ParamSpec[] = [
  { key: "temperature", label: "Temperature", min: 0, max: 2, step: 0.1, integer: false, hint: "0 is the most repeatable." },
  { key: "top_p", label: "Top-p", min: 0, max: 1, step: 0.05, integer: false, hint: "Nucleus sampling cut-off." },
  { key: "top_k", label: "Top-k", min: 0, max: 1000, step: 1, integer: true, hint: "Candidates considered per token." },
  { key: "max_tokens", label: "Max output tokens", min: 1, max: 200000, step: 1, integer: true, hint: "Stops generation at this length." },
  { key: "seed", label: "Seed", min: 0, max: 2147483647, step: 1, integer: true, hint: "Same seed and settings repeat a result where the runtime supports it." },
  { key: "repeat_penalty", label: "Repeat penalty", min: 0, max: 3, step: 0.05, integer: false, hint: "Above 1 discourages repetition." },
];

/** Read a number field: empty means "not set"; anything invalid or out of range is an error. */
export function parseParam(spec: ParamSpec, text: string): { value: number | null; error?: string } {
  const t = text.trim();
  if (t === "") return { value: null };
  const v = Number(t);
  if (!Number.isFinite(v)) return { value: null, error: `${spec.label} must be a number` };
  if (spec.integer && !Number.isInteger(v)) return { value: null, error: `${spec.label} must be a whole number` };
  if (v < spec.min || v > spec.max) return { value: null, error: `${spec.label} must be between ${spec.min} and ${spec.max}` };
  return { value: v };
}

/** One line describing the settings that were actually set. */
export function describeParams(p: Params): string {
  const parts: string[] = [];
  for (const spec of PARAM_SPECS) {
    const v = p[spec.key];
    if (v !== null && v !== undefined) parts.push(`${spec.label.toLowerCase()} ${v}`);
  }
  if (p.think === false) parts.push("thinking off");
  return parts.length ? parts.join(" · ") : "runtime defaults";
}

// ---------------------------------------------------------------- model library

export interface LibraryItem {
  key: string; model_id: string; kind: string; provider_name: string | null; status: string; favorite: boolean; local: boolean;
  meta: { display_name: string | null; family: string | null; quantization: string | null; size_bytes: number | null; parameter_count: number | null; context_length: number | null; capabilities: string[] };
}

export interface LibraryFilter { query: string; source: string; status: string; favoritesOnly: boolean }

export function filterModels<T extends LibraryItem>(models: T[], f: LibraryFilter): T[] {
  const words = f.query.toLowerCase().split(/\s+/).filter(Boolean);
  return models.filter((m) => {
    if (f.favoritesOnly && !m.favorite) return false;
    if (f.source !== "all" && (m.provider_name ?? "Model folder") !== f.source) return false;
    if (f.status !== "all" && m.status !== f.status) return false;
    const haystack = [m.model_id, m.meta.display_name, m.meta.family, m.meta.quantization, m.provider_name, m.kind, ...m.meta.capabilities]
      .filter(Boolean).join(" ").toLowerCase();
    return words.every((w) => haystack.includes(w));
  });
}

export type SortKey = "name" | "size" | "provider" | "status" | "params" | "context";

const STATUS_ORDER: Record<string, number> = { installed: 0, file: 1, remote: 2, disabled: 3, missing: 4 };

/** Sort a copy of the list. Unknown sizes and counts always go last, whichever way the sort runs. */
export function sortModels<T extends LibraryItem>(models: T[], key: SortKey, descending: boolean): T[] {
  const dir = descending ? -1 : 1;
  const text = (a: string, b: string) => a.localeCompare(b, "en", { sensitivity: "base" });
  const numeric = (a: Maybe, b: Maybe) => {
    if (!known(a) && !known(b)) return 0;
    if (!known(a)) return 1;
    if (!known(b)) return -1;
    return (a - b) * dir;
  };
  return [...models].sort((a, b) => {
    let order = 0;
    if (key === "size") order = numeric(a.meta.size_bytes, b.meta.size_bytes);
    else if (key === "params") order = numeric(a.meta.parameter_count, b.meta.parameter_count);
    else if (key === "context") order = numeric(a.meta.context_length, b.meta.context_length);
    else if (key === "provider") order = text(a.provider_name ?? "Model folder", b.provider_name ?? "Model folder") * dir;
    else if (key === "status") order = ((STATUS_ORDER[a.status] ?? 9) - (STATUS_ORDER[b.status] ?? 9)) * dir;
    else order = text(a.model_id, b.model_id) * dir;
    return order !== 0 ? order : text(a.model_id, b.model_id);
  });
}

// ---------------------------------------------------------------- benchmarks

export interface StatLike { n: number; mean: number; median: number; min: number; max: number; stddev: number }

/** "median (min–max, n runs)" for a statistic, formatted by `fmt`. */
export function fmtStat(stat: StatLike | null | undefined, fmt: (v: number) => string): string {
  if (!stat || stat.n === 0) return UNKNOWN;
  if (stat.n === 1) return `${fmt(stat.median)} (1 run)`;
  return `${fmt(stat.median)} (${fmt(stat.min)}–${fmt(stat.max)}, ${stat.n} runs)`;
}

/** How much two medians differ, as "1.8× faster" style text; null when either is unknown. */
export function ratio(a: Maybe, b: Maybe): number | null {
  if (!known(a) || !known(b) || a <= 0 || b <= 0) return null;
  return a / b;
}

/** A client-side request id the core accepts (letters, digits and dashes). */
export function newRequestId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `r-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}
