// Model storage: where downloaded model files live, how much space they use, and removal of
// the files NEXUS itself downloaded.
import { FolderOpen, Trash2 } from "lucide-react";
import { useState } from "react";
import { Badge, Button, ErrorBanner, Spinner } from "../../components/ui";
import { api, type StorageInfo } from "../../lib/api";
import { fmtBytes, fmtInt } from "../../lib/lab";
import { errorText, timeAgo, useAsync } from "../../lib/util";
import { DownloadsList, useDownloads } from "./AddModel";
import { ConfirmButton, Section, type LabCtx } from "./shared";

export function StoragePanel({ ctx }: { ctx: LabCtx }) {
  const storage = useAsync(api.labStorage, []);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const downloads = useDownloads(() => {
    void storage.reload();
    void ctx.refreshLibrary();
  });

  const apply = async (fn: () => Promise<StorageInfo | null>) => {
    setBusy(true);
    setError(undefined);
    try {
      const next = await fn();
      if (next) storage.setData(next);
      await ctx.refreshLibrary();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const info = storage.data;
  if (!info) return storage.error ? <div className="p-4"><ErrorBanner message={storage.error} /></div> : <Spinner />;

  // Models held by runtimes are stored by those runtimes, not in this folder.
  const byRuntime = new Map<string, { bytes: number; count: number }>();
  for (const m of ctx.library?.models ?? []) {
    if (m.status !== "installed" || !m.provider_name || m.meta.size_bytes === null) continue;
    const entry = byRuntime.get(m.provider_name) ?? { bytes: 0, count: 0 };
    entry.bytes += m.meta.size_bytes;
    entry.count += 1;
    byRuntime.set(m.provider_name, entry);
  }
  const usedPct = info.total_bytes && info.free_bytes !== null ? Math.round(((info.total_bytes - info.free_bytes) / info.total_bytes) * 100) : null;

  return (
    <div data-testid="storage-panel" className="space-y-5 p-4">
      <ErrorBanner message={error} />
      <Section title="Model folder" hint="GGUF files NEXUS downloads are stored here. Runtimes such as Ollama keep their own models elsewhere.">
        <div className="rounded-md border border-line p-3">
          <div className="flex items-center gap-2">
            <Badge tone={info.is_default ? "neutral" : "blue"}>{info.is_default ? "default folder" : "custom folder"}</Badge>
            {!info.exists && <Badge tone="red">folder missing</Badge>}
          </div>
          <div data-testid="storage-dir" className="selectable mt-2 break-all font-mono text-xs text-dim">{info.dir}</div>
          <div className="mt-3 grid grid-cols-3 gap-2 text-xs">
            <div><div className="text-faint">Model files</div><div className="tabular-nums">{fmtBytes(info.used_bytes)}</div></div>
            <div><div className="text-faint">Free on disk</div><div className="tabular-nums">{info.free_bytes !== null ? fmtBytes(info.free_bytes) : "not reported"}</div></div>
            <div><div className="text-faint">Disk size</div><div className="tabular-nums">{info.total_bytes !== null ? `${fmtBytes(info.total_bytes)}${usedPct !== null ? ` · ${usedPct}% used` : ""}` : "not reported"}</div></div>
          </div>
          <div className="mt-3 flex gap-2">
            <Button size="sm" busy={busy} onClick={() => void apply(api.labStorageChoose)} title="Opens a folder picker"><FolderOpen size={12} /> Change folder…</Button>
            {!info.is_default && <Button size="sm" variant="ghost" disabled={busy} onClick={() => void apply(api.labStorageReset)}>Use the default folder</Button>}
          </div>
          <div className="mt-2 text-2xs leading-relaxed text-faint">Changing the folder does not move or delete files already downloaded.</div>
        </div>
      </Section>

      <Section title={`Files in the folder (${info.files.length})`}>
        {info.files.length === 0 && <div className="text-xs text-faint">No model files here yet.</div>}
        {info.files.map((f) => (
          <div key={f.name} data-file={f.name} className="flex items-center gap-2 rounded-md border border-line p-2">
            <div className="min-w-0 flex-1">
              <div className="selectable truncate font-mono text-xs">{f.name}</div>
              <div className="text-2xs text-faint">
                {fmtBytes(f.size_bytes)} ({fmtInt(f.size_bytes)} bytes){f.modified_at ? ` · ${timeAgo(f.modified_at)}` : ""}
              </div>
            </div>
            {f.partial ? <Badge tone="amber">unfinished download</Badge> : f.managed ? <Badge>downloaded by NEXUS</Badge> : <Badge tone="blue">your file</Badge>}
            {f.managed && (
              <ConfirmButton label="Delete" confirm={`Delete ${fmtBytes(f.size_bytes)} permanently`} icon={<Trash2 size={12} />} busy={busy} onConfirm={() => apply(() => api.labStorageDelete(f.name))} />
            )}
          </div>
        ))}
        {info.files.some((f) => !f.managed && !f.partial) && (
          <div className="text-2xs leading-relaxed text-faint">Files marked “your file” were not downloaded by NEXUS, so NEXUS will not delete them. Remove them with your file manager.</div>
        )}
      </Section>

      <Section title="Models held by runtimes" hint="Reported by each runtime. Remove one from its detail view in the library.">
        {byRuntime.size === 0 && <div className="text-xs text-faint">No connected runtime reports installed models.</div>}
        {[...byRuntime.entries()].map(([name, usage]) => (
          <div key={name} className="flex items-center justify-between rounded-md border border-line p-2 text-xs">
            <span>{name}</span>
            <span className="text-dim tabular-nums">{fmtBytes(usage.bytes)} in {usage.count} model{usage.count === 1 ? "" : "s"}</span>
          </div>
        ))}
      </Section>

      <Section title="Downloads">
        <DownloadsList downloads={downloads.data ?? []} reload={() => void downloads.reload()} />
      </Section>
    </div>
  );
}
