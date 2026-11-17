// Adding models: pull through Ollama, find GGUF files on Hugging Face, or download from a URL.
// Nothing here downloads anything until the user presses a download button.
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { ArrowLeft, Download as DownloadIcon, RotateCw, Search as SearchIcon, ShieldCheck, Square, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Badge, Button, ErrorBanner, Field, Input, Select } from "../../components/ui";
import { api, LAB_DOWNLOAD, type Download, type HfFile, type HfModel, type HfRepo } from "../../lib/api";
import { fmtBytes, fmtInt, percent } from "../../lib/lab";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { Progress, Section, type LabCtx } from "./shared";

type Mode = "ollama" | "hf" | "url";

const ACTIVE = ["downloading", "verifying"];
const STATUS_TONE: Record<Download["status"], "green" | "amber" | "blue" | "red" | "neutral"> = {
  downloading: "blue", verifying: "blue", completed: "green", failed: "red", cancelled: "neutral", interrupted: "amber",
};

/** The download list, kept current from the core's progress events. */
export function useDownloads(onCompleted: () => void) {
  const list = useAsync(api.labDownloads, []);
  const seen = useRef(new Map<string, string>());
  const done = useRef(onCompleted);
  done.current = onCompleted;
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const sub = listen(LAB_DOWNLOAD, () => {
      // Progress arrives several times a second; one reload per burst is enough.
      if (timer) return;
      timer = setTimeout(() => {
        timer = undefined;
        void list.reload();
      }, 250);
    });
    return () => {
      clearTimeout(timer);
      void sub.then((off) => off());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    for (const d of list.data ?? []) {
      const before = seen.current.get(d.id);
      if (before && before !== "completed" && d.status === "completed") done.current();
      seen.current.set(d.id, d.status);
    }
  }, [list.data]);
  return list;
}

export function DownloadsList({ downloads, reload }: { downloads: Download[]; reload: () => void }) {
  const [error, setError] = useState<string>();
  const act = async (fn: () => Promise<unknown>) => {
    setError(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    reload();
  };
  if (downloads.length === 0) return <div className="text-xs text-faint">No downloads yet.</div>;
  return (
    <div className="space-y-2">
      <ErrorBanner message={error} />
      {downloads.map((d) => {
        const active = ACTIVE.includes(d.status);
        const pct = percent(d.received_bytes, d.total_bytes);
        return (
          <div key={d.id} data-download={d.name} data-status={d.status} className="rounded-md border border-line p-2.5">
            <div className="flex items-center gap-2">
              <span className="selectable min-w-0 flex-1 truncate font-mono text-xs">{d.name}</span>
              <Badge tone={STATUS_TONE[d.status]}>{d.status}</Badge>
              {d.verified && d.status === "completed" && (
                <span className="inline-flex items-center gap-1 text-[11px] text-accent" title={d.kind === "ollama" ? "Ollama verified the downloaded layers" : `SHA-256 matched: ${d.sha256}`}>
                  <ShieldCheck size={11} /> verified
                </span>
              )}
            </div>
            <div className="mt-0.5 truncate text-[11px] text-faint" title={d.source}>{d.kind === "ollama" ? d.source : d.source.replace(/^https?:\/\//, "")}</div>
            {active && <div className="mt-2"><Progress value={pct} /></div>}
            <div className="mt-1.5 flex items-center gap-2 text-[11px] text-dim">
              <span className="tabular-nums">
                {active || d.status !== "completed" ? `${fmtBytes(d.received_bytes)}${d.total_bytes ? ` of ${fmtBytes(d.total_bytes)}` : ""}${pct !== null && active ? ` · ${pct}%` : ""}` : fmtBytes(d.total_bytes ?? d.received_bytes)}
              </span>
              {active && d.detail && <span className="truncate text-faint">{d.detail}</span>}
              {!active && <span className="text-faint">{timeAgo(d.finished_at ?? d.updated_at)}</span>}
              {d.status === "completed" && !d.verified && d.kind === "file" && <span className="text-faint" title={`No checksum was published for this file. Its SHA-256 is ${d.sha256 ?? "unknown"}.`}>no published checksum</span>}
              <span className="ml-auto flex items-center gap-1">
                {active && <Button size="sm" variant="ghost" onClick={() => void act(() => api.labDownloadCancel(d.id))}><Square size={11} /> Cancel</Button>}
                {["failed", "interrupted", "cancelled"].includes(d.status) && <Button size="sm" variant="ghost" onClick={() => void act(() => api.labDownloadResume(d.id))}><RotateCw size={11} /> {d.status === "cancelled" ? "Start again" : "Resume"}</Button>}
                {!active && !(d.kind === "file" && d.status === "completed") && (
                  <Button size="sm" variant="ghost" title="Remove from this list (an unfinished file is deleted)" aria-label="Remove from list" onClick={() => void act(() => api.labDownloadRemove(d.id))}><X size={12} /></Button>
                )}
              </span>
            </div>
            {d.error && <div className="selectable mt-1 text-xs text-danger">{d.error}</div>}
          </div>
        );
      })}
    </div>
  );
}

const safeName = (path: string) => (path.split("/").pop() ?? path).replace(/[^A-Za-z0-9._+-]/g, "_").replace(/^\.+/, "");

function HuggingFace({ freeBytes, start }: { freeBytes: number | null; start: (fn: () => Promise<Download>) => Promise<void> }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<HfModel[]>();
  const [repo, setRepo] = useState<HfRepo>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const tooBig = (f: HfFile) => freeBytes !== null && f.size_bytes !== null && f.size_bytes > freeBytes;

  return (
    <div className="space-y-3">
      <p className="text-xs leading-relaxed text-dim">
        Searches public repositories on Hugging Face that contain GGUF files. <span className="text-warn">Searching sends your search text to huggingface.co</span>; nothing is downloaded until you choose a file.
      </p>
      <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); void run(async () => { setRepo(undefined); setResults(await api.labHfSearch(query)); }); }}>
        <Input aria-label="Search Hugging Face" placeholder="Model name, e.g. qwen coder" value={query} onChange={(e) => setQuery(e.target.value)} />
        <Button type="submit" busy={busy} disabled={!query.trim()}><SearchIcon size={12} /> Search</Button>
      </form>
      <ErrorBanner message={error} />
      {repo ? (
        <div data-testid="hf-repo" className="space-y-2">
          <button className="flex items-center gap-1 text-xs text-accent hover:underline" onClick={() => setRepo(undefined)}><ArrowLeft size={11} /> Back to results</button>
          <div>
            <div className="selectable font-mono text-xs font-semibold">{repo.id}</div>
            <div className="mt-1 flex flex-wrap gap-1.5">
              <Badge>{repo.license ? `licence: ${repo.license}` : "licence not stated"}</Badge>
              {repo.architecture && <Badge>{repo.architecture}</Badge>}
              {repo.context_length !== null && <Badge>{fmtInt(repo.context_length)} ctx</Badge>}
              {repo.gated && <Badge tone="amber">gated</Badge>}
            </div>
            <div className="selectable mt-1 truncate text-[11px] text-faint">{repo.page_url}</div>
          </div>
          {repo.gated && <div className="text-xs text-warn">This repository asks visitors to accept its terms on Hugging Face first. NEXUS has no Hugging Face login, so these downloads will be refused.</div>}
          {repo.files.length === 0 && <div className="text-xs text-dim">This repository has no GGUF files.</div>}
          {repo.files.map((f) => (
            <div key={f.path} data-hf-file={f.path} className="flex items-center gap-2 rounded-md border border-line p-2">
              <div className="min-w-0 flex-1">
                <div className="selectable truncate font-mono text-xs">{f.path}</div>
                <div className="text-[11px] text-faint">
                  {f.size_bytes !== null ? fmtBytes(f.size_bytes) : "size not stated"} · {f.sha256 ? "SHA-256 published, will be verified" : "no checksum published"}
                  {tooBig(f) && <span className="text-danger"> · larger than the free space</span>}
                </div>
              </div>
              <Button size="sm" disabled={tooBig(f)} onClick={() => void start(() => api.labDownloadFile(f.url, safeName(f.path), f.sha256))}>
                <DownloadIcon size={12} /> Download
              </Button>
            </div>
          ))}
        </div>
      ) : results && (
        <div data-testid="hf-results" className="space-y-1">
          {results.length === 0 && <div className="text-xs text-dim">No public repository with GGUF files matches.</div>}
          {results.map((m) => (
            <button key={m.id} data-hf-repo={m.id} onClick={() => void run(async () => setRepo(await api.labHfFiles(m.id)))} className="block w-full rounded-md border border-line p-2 text-left hover:bg-raised">
              <div className="truncate font-mono text-xs">{m.id}</div>
              <div className="mt-0.5 flex gap-3 text-[11px] text-faint">
                <span>{m.downloads !== null ? `${fmtInt(m.downloads)} downloads` : "downloads not stated"}</span>
                <span>{m.license ?? "licence not stated"}</span>
                {m.gated && <span className="text-warn">gated</span>}
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function AddModel({ ctx }: { ctx: LabCtx }) {
  const ollama = ctx.providers.filter((p) => p.kind === "ollama" && p.enabled);
  const [mode, setMode] = useState<Mode>(ollama.length ? "ollama" : "hf");
  const [providerId, setProviderId] = useState(ollama[0]?.id ?? "");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [fileName, setFileName] = useState("");
  const [sha, setSha] = useState("");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const storage = useAsync(api.labStorage, []);
  const downloads = useDownloads(() => {
    void ctx.refreshLibrary();
    void storage.reload();
  });

  const start = async (fn: () => Promise<Download>) => {
    setBusy(true);
    setError(undefined);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      void downloads.reload();
    }
  };
  const target = ollama.find((p) => p.id === providerId) ?? ollama[0];

  return (
    <div data-testid="add-model" className="space-y-5 p-4">
      <Section title="Add a model" hint="Downloads only start when you press a download button. Models can be many gigabytes.">
        <div className="flex gap-1 rounded-md border border-line p-0.5 text-xs">
          {([["ollama", "Ollama pull"], ["hf", "Hugging Face"], ["url", "Direct URL"]] as const).map(([id, label]) => (
            <button key={id} aria-pressed={mode === id} onClick={() => { setMode(id); setError(undefined); }} className={clsx("flex-1 rounded px-2 py-1.5 font-medium", mode === id ? "bg-raised text-ink" : "text-dim hover:text-ink")}>{label}</button>
          ))}
        </div>
      </Section>
      <ErrorBanner message={error} />

      {mode === "ollama" && (
        target ? (
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); void start(() => api.labDownloadOllama(target.id, name)); }}>
            <p className="text-xs leading-relaxed text-dim">
              Ollama downloads the model from its registry, resumes interrupted pulls and verifies each layer itself. NEXUS relays the progress and can cancel it.
            </p>
            {ollama.length > 1 && (
              <Field label="Connection">
                <Select aria-label="Ollama connection" value={target.id} onChange={(e) => setProviderId(e.target.value)}>
                  {ollama.map((p) => <option key={p.id} value={p.id}>{p.name}{p.local ? "" : " (remote: the model is stored there)"}</option>)}
                </Select>
              </Field>
            )}
            <Field label="Model name" hint="As listed at ollama.com/library, for example a name and a tag separated by a colon.">
              <Input aria-label="Ollama model name" className="font-mono" placeholder="name:tag" value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Button type="submit" variant="primary" busy={busy} disabled={!name.trim()}><DownloadIcon size={12} /> Pull with {target.name}</Button>
          </form>
        ) : (
          <div className="space-y-2 text-xs leading-relaxed text-dim">
            <p>Pulling by name needs a connected Ollama. None is connected and enabled.</p>
            <Button size="sm" onClick={() => ctx.go("runtimes")}>Open Runtimes</Button>
          </div>
        )
      )}

      {mode === "hf" && <HuggingFace freeBytes={storage.data?.free_bytes ?? null} start={start} />}

      {mode === "url" && (
        <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); void start(() => api.labDownloadFile(url, fileName.trim() || null, sha.trim() || null)); }}>
          <p className="text-xs leading-relaxed text-dim">
            Download a <span className="font-mono">.gguf</span> file over HTTPS into the model folder. The file is checked to be a real GGUF model before it is kept.
          </p>
          <Field label="File address"><Input aria-label="File address" className="font-mono" placeholder="https://…/model.gguf" value={url} onChange={(e) => setUrl(e.target.value)} /></Field>
          <Field label="Save as" hint="Optional. Defaults to the name in the address."><Input aria-label="Save as" className="font-mono" placeholder="model.gguf" value={fileName} onChange={(e) => setFileName(e.target.value)} /></Field>
          <Field label="SHA-256" hint="Optional. If given, the download is rejected unless it matches."><Input aria-label="SHA-256" className="font-mono" placeholder="64 hexadecimal characters" value={sha} onChange={(e) => setSha(e.target.value)} /></Field>
          <Button type="submit" variant="primary" busy={busy} disabled={!url.trim()}><DownloadIcon size={12} /> Download</Button>
        </form>
      )}

      {mode !== "ollama" && storage.data && (
        <div className="text-[11px] text-faint">
          Files go to <span className="selectable font-mono">{storage.data.dir}</span>
          {storage.data.free_bytes !== null ? ` · ${fmtBytes(storage.data.free_bytes)} free` : " · free space not reported on this system"}
        </div>
      )}

      <Section title="Downloads">
        <ErrorBanner message={downloads.error} />
        <DownloadsList downloads={downloads.data ?? []} reload={() => void downloads.reload()} />
      </Section>
    </div>
  );
}
