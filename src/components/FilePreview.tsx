import { lazy, Suspense } from "react";
import { api } from "../lib/api";
import { useAsync } from "../lib/util";
import { Empty, Spinner } from "./ui";

const CodeViewer = lazy(() => import("./CodeViewer"));

/** Loads a project file and shows it in the read-only editor. */
export function FilePreview({ projectId, path, line }: { projectId: string; path: string; line?: number }) {
  const { data, error, loading } = useAsync(() => api.readFile(projectId, path), [projectId, path]);
  if (error) return <Empty title="Cannot preview this file">{error}</Empty>;
  if (loading || data === undefined) return <Spinner />;
  return (
    <div className="flex h-full flex-col">
      <div className="selectable shrink-0 border-b border-line px-3 py-1.5 font-mono text-xs text-dim">{path}</div>
      <div className="min-h-0 flex-1">
        <Suspense fallback={<Spinner />}>
          <CodeViewer path={`${projectId}/${path}`} value={data} line={line} />
        </Suspense>
      </div>
    </div>
  );
}
