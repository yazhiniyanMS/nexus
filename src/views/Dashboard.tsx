import { FolderPlus } from "lucide-react";
import { useState } from "react";
import type { Workspace } from "../App";
import { Badge, Button, Card, Empty, ErrorBanner, PageHeader } from "../components/ui";
import { api } from "../lib/api";
import { importProject } from "../lib/importProject";
import { errorText, formatNumber, primaryLanguage, timeAgo, useAsync } from "../lib/util";

function Stat({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <Card className="p-4">
      <div className="text-xs text-dim">{label}</div>
      <div className="mt-1 text-2xl font-semibold tabular-nums">{value}</div>
      {detail && <div className="mt-0.5 text-xs text-faint">{detail}</div>}
    </Card>
  );
}

export default function Dashboard({ ws }: { ws: Workspace }) {
  const stats = useAsync(api.getStats, [ws.projects.length]);
  const activity = useAsync(() => api.listActivity(12), [ws.projects.length]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const onImport = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const p = await importProject();
      if (p) {
        await ws.reloadProjects();
        ws.selectProject(p.id);
        ws.go("projects");
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const s = stats.data;
  return (
    <>
      <PageHeader
        title="Overview"
        subtitle="Your local developer workspace"
        actions={
          <Button variant="primary" onClick={onImport} busy={busy}>
            <FolderPlus size={14} /> Import project
          </Button>
        }
      />
      <div className="flex-1 space-y-5 overflow-auto p-5">
        <ErrorBanner message={error ?? stats.error} />
        <div className="grid grid-cols-4 gap-3">
          <Stat label="Projects" value={formatNumber(s?.projects ?? 0)} detail={`${s?.analyzed_projects ?? 0} analysed`} />
          <Stat label="Tasks" value={formatNumber(s?.tasks_total ?? 0)} detail={`${s?.tasks_active ?? 0} active · ${s?.tasks_completed ?? 0} completed`} />
          <Stat label="AI providers" value={formatNumber(s?.providers ?? 0)} detail={s?.providers ? "configured" : "optional — none configured"} />
          <Stat
            label="Tokens used"
            value={formatNumber((s?.input_tokens ?? 0) + (s?.output_tokens ?? 0))}
            detail={`${formatNumber(s?.input_tokens ?? 0)} in · ${formatNumber(s?.output_tokens ?? 0)} out`}
          />
        </div>

        <div className="grid grid-cols-2 gap-5">
          <Card>
            <div className="border-b border-line px-4 py-2.5 text-[13px] font-semibold">Projects</div>
            {ws.projects.length === 0 ? (
              <Empty title="No projects yet">
                Import a local repository to analyse its structure, dependencies and Git history. Nothing leaves your machine.
              </Empty>
            ) : (
              <ul className="divide-y divide-line">
                {ws.projects.slice(0, 8).map((p) => (
                  <li key={p.id}>
                    <button
                      className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-raised/60"
                      onClick={() => {
                        ws.selectProject(p.id);
                        ws.go("projects");
                      }}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] font-medium">{p.name}</div>
                        <div className="truncate text-xs text-faint">{p.path}</div>
                      </div>
                      {!p.exists ? (
                        <Badge tone="red">missing</Badge>
                      ) : (
                        p.analysis?.languages.length ? <Badge>{primaryLanguage(p.analysis.languages)}</Badge> : null
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Card>

          <Card>
            <div className="border-b border-line px-4 py-2.5 text-[13px] font-semibold">Recent activity</div>
            {!activity.data?.length ? (
              <Empty title="Nothing yet">Imports, analyses, task runs and AI requests are recorded here.</Empty>
            ) : (
              <ul className="divide-y divide-line">
                {activity.data.map((a) => (
                  <li key={a.id} className="flex items-center gap-3 px-4 py-2">
                    <Badge>{a.kind}</Badge>
                    <span className="min-w-0 flex-1 truncate text-[13px]">{a.message}</span>
                    <span className="shrink-0 text-xs text-faint">{timeAgo(a.at)}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>
      </div>
    </>
  );
}
