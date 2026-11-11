import { Activity as ActivityIcon } from "lucide-react";
import { Badge, Button, Empty, ErrorBanner, PageHeader, Spinner } from "../components/ui";
import { api } from "../lib/api";
import { confirmAction } from "../lib/importProject";
import { useAsync } from "../lib/util";

export default function ActivityView() {
  const { data, error, loading, reload } = useAsync(() => api.listActivity(500), []);
  return (
    <>
      <PageHeader
        title="Activity"
        subtitle="A local record of what happened in this workspace"
        actions={
          <>
            <Button onClick={() => void reload()}>Refresh</Button>
            <Button
              variant="danger"
              disabled={!data?.length}
              onClick={async () => {
                if (await confirmAction("Clear the whole activity log? This cannot be undone.")) {
                  await api.clearActivity();
                  await reload();
                }
              }}
            >
              Clear log
            </Button>
          </>
        }
      />
      <div className="flex-1 overflow-auto">
        <div className="p-4 empty:hidden"><ErrorBanner message={error} /></div>
        {loading && !data ? (
          <Spinner />
        ) : !data?.length ? (
          <Empty icon={<ActivityIcon size={28} />} title="No activity recorded">
            Project imports, analyses, executed commands, task results and AI requests appear here.
          </Empty>
        ) : (
          <table className="w-full text-[13px]">
            <tbody className="divide-y divide-line">
              {data.map((a) => (
                <tr key={a.id}>
                  <td className="w-44 px-5 py-2 whitespace-nowrap text-xs text-faint tabular-nums">
                    {new Date(a.at).toLocaleString()}
                  </td>
                  <td className="w-24 py-2"><Badge>{a.kind}</Badge></td>
                  <td className="selectable py-2 pr-3">{a.message}</td>
                  <td className="px-5 py-2 text-right text-xs text-dim">{a.project_name}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
