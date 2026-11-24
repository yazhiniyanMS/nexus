import clsx from "clsx";
import { Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ViewId, Workspace } from "../App";
import { api, type SymbolHit } from "../lib/api";
import { shortcut } from "../lib/platform";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  group: string;
  run: () => void;
}

/** Searchable list of every navigation target, project, action and (live) code symbol. */
export function CommandPalette({ ws, views, actions, onClose }: {
  ws: Workspace; views: { id: ViewId; label: string }[]; actions: Command[]; onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [symbols, setSymbols] = useState<SymbolHit[]>([]);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const q = query.trim();
    if (q.length < 2 || !ws.projectId) return setSymbols([]);
    let live = true;
    const timer = setTimeout(() => {
      api.searchSymbols(ws.projectId, q, null, 8).then((r) => live && setSymbols(r)).catch(() => live && setSymbols([]));
    }, 120);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, ws.projectId]);

  const commands = useMemo(() => {
    const q = query.trim().toLowerCase();
    const fixed: Command[] = [
      ...views.map((v, i) => ({ id: `view:${v.id}`, label: `Go to ${v.label}`, hint: i < 9 ? shortcut(String(i + 1)) : undefined, group: "Navigate", run: () => ws.go(v.id) })),
      ...actions,
      ...ws.projects.filter((p) => p.id !== ws.projectId).map((p) => ({ id: `project:${p.id}`, label: `Switch to ${p.name}`, hint: p.path, group: "Projects", run: () => ws.selectProject(p.id) })),
    ].filter((c) => !q || c.label.toLowerCase().includes(q));
    const found: Command[] = symbols.map((s) => ({
      id: `symbol:${s.path}:${s.line}`,
      label: s.name,
      hint: `${s.kind} · ${s.path}:${s.line}`,
      group: "Symbols",
      run: () => ws.peek({ projectId: s.project_id, path: s.path, line: s.line }),
    }));
    // Screens and actions first: they are few, and the highlighted one must not move when symbols arrive.
    return [...fixed, ...found];
  }, [query, symbols, views, actions, ws]);

  useEffect(() => setIndex(0), [query, symbols.length]);
  useEffect(() => list.current?.querySelector("[data-active=true]")?.scrollIntoView({ block: "nearest" }), [index]);

  const run = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };

  return (
    <div className="absolute inset-0 z-40 flex items-start justify-center bg-black/55 pt-[14vh]" onMouseDown={onClose} data-testid="palette">
      <div className="w-[560px] overflow-hidden rounded-lg border border-line-strong bg-panel shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2.5 border-b border-line px-3.5">
          <Search size={15} className="text-faint" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search commands, projects and symbols…"
            className="h-11 flex-1 bg-transparent text-sm placeholder:text-faint focus:outline-none"
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setIndex((i) => Math.min(commands.length - 1, i + 1)); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setIndex((i) => Math.max(0, i - 1)); }
              else if (e.key === "Enter") { e.preventDefault(); run(commands[index]); }
              else if (e.key === "Escape") onClose();
            }}
          />
        </div>
        <div ref={list} role="listbox" aria-label="Commands" className="max-h-[46vh] overflow-auto p-1.5">
          {commands.length === 0 && <div className="px-3 py-6 text-center text-sm text-faint">Nothing matches “{query}”.</div>}
          {commands.map((c, i) => (
            <div key={c.id}>
              {(i === 0 || commands[i - 1].group !== c.group) && <div className="px-2.5 pt-2 pb-1 text-2xs font-medium text-faint">{c.group}</div>}
              <button
                role="option"
                aria-selected={i === index}
                data-active={i === index}
                tabIndex={-1}
                // Only a real pointer movement changes the highlight: rows re-rendering under a
                // resting pointer must not take the keyboard selection away.
                onMouseMove={() => i !== index && setIndex(i)}
                onClick={() => run(c)}
                className={clsx("flex w-full items-center gap-3 rounded-md px-2.5 py-1.5 text-left text-sm", i === index ? "bg-raised text-ink" : "text-dim")}
              >
                <span className={clsx("shrink-0", c.group === "Symbols" && "font-mono")}>{c.label}</span>
                {c.hint && <span className="ml-auto truncate font-mono text-2xs text-faint">{c.hint}</span>}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
