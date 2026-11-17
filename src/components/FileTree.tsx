import clsx from "clsx";
import { ChevronDown, ChevronRight, File, Folder } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type DirEntry } from "../lib/api";

function Node({ projectId, entry, depth, selected, onOpen }: {
  projectId: string; entry: DirEntry; depth: number; selected: string | null; onOpen: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [children, setChildren] = useState<DirEntry[] | null>(null);

  const toggle = async () => {
    if (!entry.is_dir) return onOpen(entry.path);
    setOpen(!open);
    if (!children) setChildren(await api.listDir(projectId, entry.path).catch(() => []));
  };

  return (
    <>
      <button
        onClick={toggle}
        style={{ paddingLeft: 8 + depth * 14 }}
        className={clsx(
          "flex h-6.5 w-full items-center gap-1.5 pr-2 text-left text-[12.5px] hover:bg-raised",
          selected === entry.path ? "bg-raised text-ink" : "text-dim",
        )}
      >
        {entry.is_dir ? (open ? <ChevronDown size={13} /> : <ChevronRight size={13} />) : <span className="w-[13px]" />}
        {entry.is_dir ? <Folder size={13} className="text-lav/80" /> : <File size={13} className="text-faint" />}
        <span className="truncate">{entry.name}</span>
      </button>
      {open && children?.map((c) => (
        <Node key={c.path} projectId={projectId} entry={c} depth={depth + 1} selected={selected} onOpen={onOpen} />
      ))}
    </>
  );
}

export function FileTree({ projectId, selected, onOpen }: { projectId: string; selected: string | null; onOpen: (path: string) => void }) {
  const [root, setRoot] = useState<DirEntry[]>([]);
  useEffect(() => {
    let live = true;
    api.listDir(projectId, "").then((r) => live && setRoot(r)).catch(() => live && setRoot([]));
    return () => {
      live = false;
    };
  }, [projectId]);
  return (
    <div className="py-1">
      {root.map((e) => (
        <Node key={e.path} projectId={projectId} entry={e} depth={0} selected={selected} onOpen={onOpen} />
      ))}
    </div>
  );
}
