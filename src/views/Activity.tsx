// Activity: a local log of what NEXUS did, grouped by day, filterable by kind and project.
import { Activity as ActivityIcon, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { ConfirmButton, Empty, ErrorBanner, IconButton, Select, Spinner, ViewHeader } from "../components/ui";
import { api, type ActivityEntry } from "../lib/api";
import { useAsync } from "../lib/util";

const dayKey = (iso: string) => new Date(iso).toDateString();

function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const yesterday = new Date(Date.now() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
}

export default function ActivityView() {
  const { data, error, loading, reload } = useAsync(() => api.listActivity(1000), []);
  const [kind, setKind] = useState("");
  const [project, setProject] = useState("");

  const kinds = useMemo(() => [...new Set((data ?? []).map((a) => a.kind))].sort(), [data]);
  const projects = useMemo(() => [...new Set((data ?? []).map((a) => a.project_name).filter((p): p is string => !!p))].sort(), [data]);
  const shown = useMemo(() => (data ?? []).filter((a) => (!kind || a.kind === kind) && (!project || a.project_name === project)), [data, kind, project]);
  const days = useMemo(() => {
    const out: { key: string; label: string; items: ActivityEntry[] }[] = [];
    for (const a of shown) {
      const key = dayKey(a.at);
      if (out[out.length - 1]?.key !== key) out.push({ key, label: dayLabel(a.at), items: [] });
      out[out.length - 1].items.push(a);
    }
    return out;
  }, [shown]);

  return (
    <>
      <ViewHeader
        title="Activity"
        context={data ? <span>{shown.length === data.length ? `${data.length} entries` : `${shown.length} of ${data.length} entries`}</span> : undefined}
        actions={
          <>
            <Select aria-label="Kind" className="h-7! w-36! text-xs" value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="">Every kind</option>
              {kinds.map((k) => <option key={k} value={k}>{k}</option>)}
            </Select>
            <Select aria-label="Project" className="h-7! w-44! text-xs" value={project} onChange={(e) => setProject(e.target.value)}>
              <option value="">Every project</option>
              {projects.map((p) => <option key={p} value={p}>{p}</option>)}
            </Select>
            <IconButton label="Refresh" onClick={() => void reload()}><RefreshCw size={14} /></IconButton>
            <ConfirmButton
              label="Clear log"
              disabled={!data?.length}
              confirm="Clear the whole log — cannot be undone"
              onConfirm={async () => {
                await api.clearActivity();
                await reload();
              }}
            />
          </>
        }
      />
      <div className="flex-1 overflow-auto">
        <div className="p-4 empty:hidden"><ErrorBanner message={error} /></div>
        {loading && !data ? (
          <Spinner />
        ) : !data?.length ? (
          <Empty icon={<ActivityIcon size={28} />} title="No activity recorded">
            Imports, analyses, commands you approve, agent tasks, commits and model requests are logged here, on this machine only.
          </Empty>
        ) : shown.length === 0 ? (
          <Empty title="Nothing matches these filters" />
        ) : (
          <div className="mx-auto max-w-5xl px-6 py-4" data-testid="activity-days">
            {days.map((d) => (
              <section key={d.key} className="mb-5">
                <h2 className="sticky top-0 z-10 bg-bg py-1.5 text-xs font-semibold text-dim">{d.label}</h2>
                <table className="w-full text-sm">
                  <tbody className="divide-y divide-line/60">
                    {d.items.map((a) => (
                      <tr key={a.id}>
                        <td className="w-20 py-1.5 align-top text-xs text-faint tabular-nums">{new Date(a.at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</td>
                        <td className="w-24 py-1.5 align-top text-xs text-dim">{a.kind}</td>
                        <td className="selectable py-1.5 pr-3">{a.message}</td>
                        <td className="w-48 truncate py-1.5 text-right align-top text-xs text-faint">{a.project_name}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            ))}
          </div>
        )}
      </div>
    </>
  );
}
