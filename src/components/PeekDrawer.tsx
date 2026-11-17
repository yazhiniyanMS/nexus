import { baseName } from "../lib/platform";
import { ArrowDownLeft, ArrowUpRight, MessageSquareText, X } from "lucide-react";
import type { Workspace } from "../App";
import { api } from "../lib/api";
import { useAsync } from "../lib/util";
import { FilePreview } from "./FilePreview";

/** Slide-over file viewer any screen can open. Shows the file plus how it connects to the rest of the code. */
export function PeekDrawer({ ws }: { ws: Workspace }) {
  const t = ws.peekTarget;
  const ctx = useAsync(() => (t ? api.fileContext(t.projectId, t.path) : Promise.resolve(undefined)), [t?.projectId, t?.path]);
  if (!t) return null;
  const c = ctx.data;
  const open = (path: string) => ws.peek({ projectId: t.projectId, path });
  return (
    <div className="absolute inset-0 z-30 flex" data-testid="peek">
      <button aria-label="Close file" className="flex-1 bg-black/50" onClick={() => ws.peek(null)} />
      <aside className="flex w-[62%] min-w-[520px] flex-col border-l border-line bg-bg shadow-2xl">
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold">{baseName(t.path)}</span>
          <button
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-dim hover:bg-raised hover:text-ink"
            onClick={() => {
              ws.askAbout(t.path);
              ws.peek(null);
            }}
          >
            <MessageSquareText size={13} /> Ask about this file
          </button>
          <button aria-label="Close" className="grid size-7 place-items-center rounded-md text-dim hover:bg-raised hover:text-ink" onClick={() => ws.peek(null)}>
            <X size={15} />
          </button>
        </div>
        {c && (c.imports.length > 0 || c.imported_by.length > 0 || c.symbols.length > 0) && (
          <div className="max-h-40 shrink-0 space-y-1.5 overflow-auto border-b border-line px-3 py-2 text-xs" data-testid="peek-connections">
            {c.imports.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="flex items-center gap-1 text-faint"><ArrowUpRight size={12} /> Imports</span>
                {c.imports.map((i) =>
                  i.external ? (
                    <span key={i.target} className="rounded border border-line px-1.5 py-px font-mono text-faint">{i.target}</span>
                  ) : (
                    <button key={i.target} onClick={() => open(i.target)} className="rounded border border-line bg-raised px-1.5 py-px font-mono text-lav hover:border-lav/50">
                      {i.target}
                    </button>
                  ),
                )}
              </div>
            )}
            {c.imported_by.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="flex items-center gap-1 text-faint"><ArrowDownLeft size={12} /> Used by</span>
                {c.imported_by.map((p) => (
                  <button key={p} onClick={() => open(p)} className="rounded border border-line bg-raised px-1.5 py-px font-mono text-lav hover:border-lav/50">
                    {p}
                  </button>
                ))}
              </div>
            )}
            {c.symbols.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-faint">Defines</span>
                {c.symbols.slice(0, 40).map((s) => (
                  <button key={s.name + s.line} title={s.signature} onClick={() => ws.peek({ projectId: t.projectId, path: t.path, line: s.line })} className="rounded px-1 font-mono text-dim hover:bg-raised hover:text-ink">
                    {s.name}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        <div className="min-h-0 flex-1">
          <FilePreview projectId={t.projectId} path={t.path} line={t.line} />
        </div>
      </aside>
    </div>
  );
}
