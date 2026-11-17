import { FileWarning } from "lucide-react";
import { lazy, Suspense } from "react";
import { api } from "../lib/api";
import { formatBytes, useAsync } from "../lib/util";
import { Empty, Spinner } from "./ui";

const CodeViewer = lazy(() => import("./CodeViewer"));

/** Loads a project file and shows it in the read-only editor. */
export function FilePreview({ projectId, path, line }: { projectId: string; path: string; line?: number }) {
  const result = useAsync(() => api.readFile(projectId, path), [projectId, path]);
  const { error, loading } = result;
  // Never show details of a previously opened file while another loads or fails.
  const data = error || loading ? undefined : result.data;
  return (
    <div className="flex h-full flex-col" data-testid="file-preview">
      <div className="selectable flex shrink-0 items-center gap-3 border-b border-line px-3 py-1.5 font-mono text-xs text-dim">
        <span className="truncate">{path}</span>
        {data && <span className="ml-auto shrink-0 text-faint">{formatBytes(data.size)}</span>}
      </div>
      {data?.truncated && (
        <div className="shrink-0 border-b border-warn/30 bg-warn/10 px-3 py-1.5 text-xs text-warn">
          Large file: showing the first {formatBytes(data.content.length)} of {formatBytes(data.size)}.
        </div>
      )}
      <div className="min-h-0 flex-1">
        {error ? (
          <Empty icon={<FileWarning size={26} />} title="Cannot preview this file">{error}</Empty>
        ) : loading || !data ? (
          <Spinner />
        ) : (
          <Suspense fallback={<Spinner />}>
            <CodeViewer path={`${projectId}/${path}`} value={data.content} line={line} />
          </Suspense>
        )}
      </div>
    </div>
  );
}
