import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { listen } from "@tauri-apps/api/event";
import clsx from "clsx";
import { Columns2, Plus, RotateCw, Rows2, Search as SearchIcon, Square, TerminalSquare, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Workspace } from "../App";
import { RuntimeChip } from "../components/RuntimeChip";
import { baseName, isMac } from "../lib/platform";
import { Badge, Button, Empty, ErrorBanner, Input, Spinner } from "../components/ui";
import { api, TERM_CHANGED, TERM_OUTPUT, type TermMatch, type TermOutput, type TermSession } from "../lib/api";
import {
  addTab, emptyLayout, leaves, parseLayout, place, reconcile, removePane, replacePane, setRatio, splitPane, tabOf,
  type Divider, type Rect, type TermLayout,
} from "../lib/termLayout";
import { errorText } from "../lib/util";

const THEME = {
  background: "#060607", foreground: "#ededee", cursor: "#86dfc4", cursorAccent: "#060607",
  selectionBackground: "#b4a8f244", black: "#141417", brightBlack: "#63636b",
  red: "#f87171", brightRed: "#fca5a5", green: "#86dfc4", brightGreen: "#6ee7b7",
  yellow: "#fbbf24", brightYellow: "#fcd34d", blue: "#60a5fa", brightBlue: "#93c5fd",
  magenta: "#c084fc", brightMagenta: "#d8b4fe", cyan: "#22d3ee", brightCyan: "#67e8f9",
  white: "#d4d4d8", brightWhite: "#fafafa",
};
const FONT = 'ui-monospace, "SF Mono", "Cascadia Code", Menlo, Consolas, monospace';

/** What the screen can ask a mounted pane to do. */
interface PaneApi { focus: () => void; find: (query: string, back?: boolean) => boolean }

function statusBadge(s: TermSession) {
  if (s.status === "running") return <Badge tone="green">running</Badge>;
  if (s.status === "ended") return <Badge tone="amber">ended when NEXUS closed</Badge>;
  if (s.exit_code === null) return <Badge>stopped</Badge>;
  return <Badge tone={s.exit_code === 0 ? "neutral" : "red"}>exited · code {s.exit_code}</Badge>;
}

// ------------------------------------------------------------------ one terminal

function Pane({ session, box, focused, onFocus, register, actions }: {
  session: TermSession;
  /** Where the pane sits in the tab area, or null while its tab is hidden. */
  box: Rect | null;
  focused: boolean;
  onFocus: () => void;
  register: (id: string, pane: PaneApi | null) => void;
  actions: { split: (dir: "row" | "col") => void; kill: () => void; restart: () => void; close: () => void; rename: (title: string) => void };
}) {
  const host = useRef<HTMLDivElement>(null);
  const [renaming, setRenaming] = useState<string>();
  const [confirmClose, setConfirmClose] = useState(false);
  const [error, setError] = useState<string>();
  const running = session.status === "running";
  // The effect below must not restart when the status changes, so it reads it through a ref.
  const live = useRef(running);
  live.current = running;

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const term = new Terminal({ fontFamily: FONT, fontSize: 12.5, lineHeight: 1.25, theme: THEME, cursorBlink: true, scrollback: 8000 });
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.open(el);
    // On Windows and Linux, Control+C is the interrupt key, so copy and paste use
    // Control+Shift+C / Control+Shift+V as other terminals there do. macOS keeps Command+C / V.
    term.attachCustomKeyEventHandler((e) => {
      if (isMac || e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
      const key = e.key.toLowerCase();
      if (key === "c") {
        const picked = term.getSelection();
        if (picked) void navigator.clipboard.writeText(picked).catch(() => undefined);
        e.preventDefault();
        return false;
      }
      // Returning false lets the webview's own paste run, which xterm turns into terminal input.
      return key !== "v";
    });
    const refit = () => {
      // Hidden panes (inactive tabs) have no size; fitting them would shrink the shell to nothing.
      if (el.clientWidth > 40 && el.clientHeight > 20) {
        try {
          fit.fit();
        } catch {
          /* the pane is being torn down */
        }
      }
    };
    refit();

    let disposed = false;
    let replayed = false;
    let end = 0;
    const queued: TermOutput[] = [];
    // Subscribe first, then load what was already printed; `end` tells the two apart.
    const off = listen<TermOutput>(TERM_OUTPUT, (e) => {
      if (e.payload.id !== session.id) return;
      if (!replayed) queued.push(e.payload);
      else if (e.payload.end > end) term.write(e.payload.data);
    });
    api.termScrollback(session.id).then((saved) => {
      if (disposed) return;
      end = saved.end;
      // Replayed output can contain terminal queries; their answers must not be typed into the
      // shell, so input is forwarded only once the replay has been processed.
      term.write(saved.data, () => {
        for (const chunk of queued) if (chunk.end > end) term.write(chunk.data);
        replayed = true;
        if (live.current) void api.termResize(session.id, term.cols, term.rows).catch(() => undefined);
      });
    }).catch((e) => !disposed && setError(errorText(e)));

    const subs = [
      term.onData((data) => {
        if (replayed && live.current) api.termWrite(session.id, data).catch((e) => setError(errorText(e)));
      }),
      term.onResize(({ cols, rows }) => {
        if (live.current) void api.termResize(session.id, cols, rows).catch(() => undefined);
      }),
    ];
    const observer = new ResizeObserver(refit);
    observer.observe(el);
    register(session.id, {
      focus: () => term.focus(),
      find: (query, back) => (back ? search.findPrevious(query) : search.findNext(query)),
    });
    return () => {
      disposed = true;
      register(session.id, null);
      observer.disconnect();
      subs.forEach((s) => s.dispose());
      void off.then((stop) => stop());
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id]);

  const icon = "grid size-6 place-items-center rounded text-faint hover:bg-raised hover:text-ink";
  return (
    <div
      data-term={session.id}
      data-status={session.status}
      onMouseDown={onFocus}
      style={box ? { left: `${box.x * 100}%`, top: `${box.y * 100}%`, width: `${box.w * 100}%`, height: `${box.h * 100}%` } : { display: "none" }}
      className="absolute flex flex-col p-0.5"
    >
      <div className={clsx("flex min-h-0 min-w-0 flex-1 flex-col border", focused ? "border-accent/50" : "border-line")}>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-line bg-panel px-2">
        {renaming === undefined ? (
          <span className="truncate text-xs font-medium" title="Double-click to rename" onDoubleClick={() => setRenaming(session.title)}>{session.title}</span>
        ) : (
          <input
            autoFocus
            value={renaming}
            onChange={(e) => setRenaming(e.target.value)}
            onBlur={() => setRenaming(undefined)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter" && renaming.trim()) {
                actions.rename(renaming.trim());
                setRenaming(undefined);
              } else if (e.key === "Escape") setRenaming(undefined);
            }}
            className="h-6 w-40 rounded border border-line bg-bg px-1.5 text-xs focus:border-lav/60 focus:outline-none"
          />
        )}
        <span className="truncate font-mono text-2xs text-faint">{session.cwd ? `./${session.cwd}` : "."}</span>
        {statusBadge(session)}
        <div className="ml-auto flex items-center gap-0.5">
          {confirmClose ? (
            <>
              <span className="text-2xs text-warn">Stop the shell and close?</span>
              <Button size="sm" variant="danger" onClick={actions.close}>Stop and close</Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirmClose(false)}>Keep</Button>
            </>
          ) : (
            <>
              <button className={icon} title="Split right" onClick={() => actions.split("row")}><Columns2 size={13} /></button>
              <button className={icon} title="Split down" onClick={() => actions.split("col")}><Rows2 size={13} /></button>
              {running
                ? <button className={icon} title="Stop the shell and everything it started" onClick={actions.kill}><Square size={12} /></button>
                : <button className={icon} title="Start a new shell here" onClick={actions.restart}><RotateCw size={12} /></button>}
              <button className={icon} title="Close" onClick={() => (running ? setConfirmClose(true) : actions.close())}><X size={13} /></button>
            </>
          )}
        </div>
      </div>
      {error && <div className="border-b border-line px-2 py-1 text-xs text-danger">{error}</div>}
      <div ref={host} className="selectable min-h-0 flex-1 overflow-hidden bg-bg px-1.5 pt-1" />
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ split divider

function SplitHandle({ divider, area, onRatio }: { divider: Divider; area: React.RefObject<HTMLDivElement | null>; onRatio: (ratio: number) => void }) {
  const row = divider.dir === "row";
  const { box, ratio } = divider;
  const drag = (e: React.MouseEvent) => {
    e.preventDefault();
    const rect = area.current?.getBoundingClientRect();
    if (!rect) return;
    const move = (m: MouseEvent) => onRatio(row
      ? (m.clientX - rect.left - box.x * rect.width) / (box.w * rect.width)
      : (m.clientY - rect.top - box.y * rect.height) / (box.h * rect.height));
    const stop = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
  };
  const style = row
    ? { left: `calc(${(box.x + box.w * ratio) * 100}% - 3px)`, top: `${box.y * 100}%`, width: 6, height: `${box.h * 100}%` }
    : { top: `calc(${(box.y + box.h * ratio) * 100}% - 3px)`, left: `${box.x * 100}%`, height: 6, width: `${box.w * 100}%` };
  return (
    <div
      data-divider={divider.path}
      onMouseDown={drag}
      title="Drag to resize"
      style={style}
      className={clsx("absolute z-10 rounded-full hover:bg-accent/60", row ? "cursor-col-resize" : "cursor-row-resize")}
    />
  );
}

// ------------------------------------------------------------------ screen

export default function TerminalView({ ws }: { ws: Workspace }) {
  const projectId = ws.projectId;
  const [sessions, setSessions] = useState<TermSession[]>();
  const [layout, setLayout] = useState<TermLayout>(emptyLayout);
  const [focused, setFocused] = useState<string>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  // Shells this system offers (the default first) and the one new terminals use.
  const [shells, setShells] = useState<string[]>([]);
  const [shell, setShell] = useState<string | null>(null);
  useEffect(() => {
    api.termShells().then(setShells).catch(() => undefined);
  }, []);
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<TermMatch[]>();
  const panes = useRef(new Map<string, PaneApi>());
  const area = useRef<HTMLDivElement>(null);
  // Which project the loaded layout belongs to, so a layout is never saved under another project.
  const loadedFor = useRef<string | null>(null);

  const register = useCallback((id: string, pane: PaneApi | null) => {
    if (pane) panes.current.set(id, pane);
    else panes.current.delete(id);
  }, []);

  const reloadSessions = useCallback(async () => {
    if (!projectId) return;
    try {
      const list = await api.termList(projectId);
      setSessions(list);
      setLayout((l) => reconcile(l, list.map((s) => s.id)));
    } catch (e) {
      setError(errorText(e));
    }
  }, [projectId]);

  // Load sessions and the saved layout whenever the project changes.
  useEffect(() => {
    loadedFor.current = null;
    setSessions(undefined);
    setLayout(emptyLayout());
    setFocused(undefined);
    setMatches(undefined);
    setError(undefined);
    if (!projectId) return;
    let live = true;
    Promise.all([api.termList(projectId), api.termLayoutGet(projectId)]).then(([list, saved]) => {
      if (!live) return;
      const next = reconcile(parseLayout(saved), list.map((s) => s.id));
      setSessions(list);
      setLayout(next);
      const tab = next.tabs.find((t) => t.id === next.active);
      setFocused(tab ? leaves(tab.root)[0] : undefined);
      loadedFor.current = projectId;
    }).catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [projectId]);

  useEffect(() => {
    const sub = listen(TERM_CHANGED, () => void reloadSessions());
    return () => void sub.then((off) => off());
  }, [reloadSessions]);

  // Save the layout shortly after it changes.
  useEffect(() => {
    if (!projectId || loadedFor.current !== projectId) return;
    const timer = setTimeout(() => void api.termLayoutSet(projectId, JSON.stringify(layout)).catch(() => undefined), 400);
    return () => clearTimeout(timer);
  }, [layout, projectId]);

  // Give the keyboard to the focused pane once it is on screen.
  useEffect(() => {
    if (!focused) return;
    const timer = setTimeout(() => panes.current.get(focused)?.focus(), 60);
    return () => clearTimeout(timer);
  }, [focused, layout.active]);

  const run = async (fn: () => Promise<void>) => {
    setError(undefined);
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const byId = useMemo(() => new Map((sessions ?? []).map((s) => [s.id, s])), [sessions]);

  const open = (place: (l: TermLayout, id: string) => TermLayout, cwd: string | null = null) => run(async () => {
    if (!projectId) return;
    const s = await api.termOpen(projectId, cwd, 80, 24, shell);
    setSessions((list) => [...(list ?? []).filter((x) => x.id !== s.id), s]);
    setLayout((l) => place(l, s.id));
    setFocused(s.id);
  });

  const close = (id: string) => run(async () => {
    await api.termClose(id);
    setSessions((list) => list?.filter((s) => s.id !== id));
    setLayout((l) => {
      const next = removePane(l, id);
      if (focused === id) {
        const tab = next.tabs.find((t) => t.id === next.active);
        setFocused(tab ? leaves(tab.root)[0] : undefined);
      }
      return next;
    });
  });

  const restart = (old: TermSession) => run(async () => {
    if (!projectId) return;
    const s = await api.termOpen(projectId, old.cwd || null, 80, 24);
    await api.termClose(old.id);
    setSessions((list) => [...(list ?? []).filter((x) => x.id !== old.id && x.id !== s.id), s]);
    setLayout((l) => replacePane(l, old.id, s.id));
    setFocused(s.id);
  });

  const selectTab = (tabId: string) => {
    const tab = layout.tabs.find((t) => t.id === tabId);
    setLayout((l) => ({ ...l, active: tabId }));
    if (tab && !(focused && leaves(tab.root).includes(focused))) setFocused(leaves(tab.root)[0]);
  };

  const searchAll = () => run(async () => {
    if (!projectId || !query.trim()) return setMatches(undefined);
    setMatches(await api.termSearch(projectId, query));
    if (focused) panes.current.get(focused)?.find(query);
  });

  const jump = (m: TermMatch) => {
    const tab = tabOf(layout, m.session_id);
    if (tab) setLayout((l) => ({ ...l, active: tab.id }));
    setFocused(m.session_id);
    setMatches(undefined);
    // The pane may have just become visible; search once it has laid out.
    setTimeout(() => panes.current.get(m.session_id)?.find(m.text.slice(0, 60)), 120);
  };

  if (!projectId || !ws.project) {
    return <Empty icon={<TerminalSquare size={28} />} title="No project selected">Import a project to open a terminal in it.</Empty>;
  }
  if (!sessions) return error ? <div className="p-5"><ErrorBanner message={error} /></div> : <Spinner />;

  const running = sessions.filter((s) => s.status === "running").length;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
        <h1 className="shrink-0 text-sm font-semibold">Terminal</h1>
        <div className="min-w-0 truncate text-xs text-dim">{ws.project.name} · {running} running · {sessions.length - running} finished</div>
        <div className="ml-auto"><RuntimeChip onOpenLab={() => ws.go("models")} /></div>
        <form className="relative flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); void searchAll(); }}>
          <SearchIcon size={13} className="pointer-events-none absolute left-2.5 text-faint" />
          <Input
            value={query}
            onChange={(e) => { setQuery(e.target.value); setMatches(undefined); }}
            onKeyDown={(e) => e.stopPropagation()}
            placeholder="Search terminal output"
            className="w-64 pl-8"
            aria-label="Search terminal output"
          />
          {matches && (
            <div data-testid="term-matches" className="absolute right-0 top-9 z-20 max-h-72 w-[30rem] overflow-auto rounded-md border border-line bg-panel p-1 shadow-xl">
              <div className="flex items-center justify-between px-2 py-1 text-2xs text-faint">
                <span>{matches.length === 0 ? "No matches in saved output" : `${matches.length}${matches.length >= 200 ? "+" : ""} matching lines`}</span>
                <button type="button" className="hover:text-ink" onClick={() => setMatches(undefined)}>Close</button>
              </div>
              {matches.map((m, i) => (
                <button type="button" key={i} onClick={() => jump(m)} className="block w-full rounded px-2 py-1 text-left hover:bg-raised">
                  <span className="mr-2 text-2xs text-accent">{m.title}:{m.line}</span>
                  <span className="font-mono text-xs text-dim">{m.text}</span>
                </button>
              ))}
            </div>
          )}
        </form>
        {shells.length > 1 && (
          <select
            aria-label="Shell for new terminals"
            title="Shell for new terminals"
            value={shell ?? shells[0]}
            onChange={(e) => setShell(e.target.value === shells[0] ? null : e.target.value)}
            className="h-7 rounded-md border border-line bg-bg px-1.5 text-xs text-dim focus:border-lav/60 focus:outline-none"
          >
            {shells.map((s) => <option key={s} value={s}>{baseName(s)}</option>)}
          </select>
        )}
        <Button size="sm" variant={layout.tabs.length === 0 ? "default" : "primary"} busy={busy} onClick={() => void open(addTab)}><Plus size={13} /> New terminal</Button>
      </div>
      {error && <div className="px-4 pt-3"><ErrorBanner message={error} /></div>}

      {layout.tabs.length === 0 ? (
        <Empty icon={<TerminalSquare size={28} />} title="No terminals yet">
          <p className="mb-3 max-w-md">
            Open your own shell in {ws.project.name}. Only what you type here is run; agents and AI cannot send input to a terminal.
            Output is saved on this machine so you can search it and come back to it.
          </p>
          <Button variant="primary" busy={busy} onClick={() => void open(addTab)}><Plus size={13} /> New terminal</Button>
        </Empty>
      ) : (
        <>
          <div className="flex h-9 shrink-0 items-end gap-1 overflow-x-auto border-b border-line px-2" role="tablist">
            {layout.tabs.map((tab) => {
              const ids = leaves(tab.root);
              const first = byId.get(ids[0]);
              const alive = ids.some((id) => byId.get(id)?.status === "running");
              return (
                <button
                  key={tab.id}
                  role="tab"
                  aria-selected={tab.id === layout.active}
                  onClick={() => selectTab(tab.id)}
                  className={clsx(
                    "flex h-8 max-w-56 shrink-0 items-center gap-2 rounded-t-md border border-b-0 px-3 text-xs",
                    tab.id === layout.active ? "border-line bg-raised text-ink" : "border-transparent text-dim hover:text-ink",
                  )}
                >
                  <span className={clsx("size-1.5 shrink-0 rounded-full", alive ? "bg-accent" : "bg-faint")} />
                  <span className="truncate">{first?.title ?? "terminal"}</span>
                  {ids.length > 1 && <span className="text-faint">+{ids.length - 1}</span>}
                </button>
              );
            })}
          </div>
          {/* Every terminal stays mounted, keyed by session, so switching tabs or changing the
              layout never re-creates one. Panes of inactive tabs are hidden. */}
          <div ref={area} className="relative m-1.5 min-h-0 flex-1">
            {layout.tabs.flatMap((tab) => {
              const shown = tab.id === layout.active;
              const placed = place(tab.root);
              return [
                ...leaves(tab.root).map((id) => {
                  const s = byId.get(id);
                  return s && (
                    <Pane
                      key={id}
                      session={s}
                      box={shown ? (placed.panes.get(id) ?? null) : null}
                      focused={focused === id}
                      onFocus={() => setFocused(id)}
                      register={register}
                      actions={{
                        split: (dir) => void open((l, fresh) => splitPane(l, id, fresh, dir), s.cwd || null),
                        kill: () => void run(() => api.termKill(id)),
                        restart: () => void restart(s),
                        close: () => void close(id),
                        rename: (title) => void run(async () => {
                          await api.termRename(id, title);
                          await reloadSessions();
                        }),
                      }}
                    />
                  );
                }),
                ...(shown ? placed.dividers.map((d) => (
                  <SplitHandle key={`${tab.id}:${d.path}`} divider={d} area={area} onRatio={(ratio) => setLayout((l) => setRatio(l, tab.id, d.path, ratio))} />
                )) : []),
              ];
            })}
          </div>
        </>
      )}
    </div>
  );
}
