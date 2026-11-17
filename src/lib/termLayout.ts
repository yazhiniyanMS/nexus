// Pane layout of the Terminal screen: tabs, each holding a tree of horizontal/vertical splits
// whose leaves are terminal sessions. Pure functions; the layout is saved per project as JSON.

export type PaneNode =
  | { kind: "leaf"; id: string }
  | { kind: "split"; dir: "row" | "col"; ratio: number; a: PaneNode; b: PaneNode };
export interface TermTab { id: string; root: PaneNode }
export interface TermLayout { tabs: TermTab[]; active: string | null }

export const emptyLayout = (): TermLayout => ({ tabs: [], active: null });
const newId = () => Math.random().toString(36).slice(2, 10);
const clampRatio = (r: number) => Math.min(0.9, Math.max(0.1, r));

export function leaves(node: PaneNode): string[] {
  return node.kind === "leaf" ? [node.id] : [...leaves(node.a), ...leaves(node.b)];
}

function mapLeaf(node: PaneNode, id: string, fn: (leaf: PaneNode) => PaneNode | null): PaneNode | null {
  if (node.kind === "leaf") return node.id === id ? fn(node) : node;
  const a = mapLeaf(node.a, id, fn);
  const b = mapLeaf(node.b, id, fn);
  // A split with one side gone collapses into the other side.
  if (!a) return b;
  if (!b) return a;
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

const mapTabs = (layout: TermLayout, fn: (root: PaneNode) => PaneNode | null): TermLayout => {
  const tabs = layout.tabs.flatMap((t) => {
    const root = fn(t.root);
    return root ? [{ ...t, root }] : [];
  });
  const active = tabs.some((t) => t.id === layout.active) ? layout.active : (tabs[tabs.length - 1]?.id ?? null);
  return { tabs, active };
};

export function addTab(layout: TermLayout, sessionId: string): TermLayout {
  const tab: TermTab = { id: newId(), root: { kind: "leaf", id: sessionId } };
  return { tabs: [...layout.tabs, tab], active: tab.id };
}

/** Put `sessionId` beside (`row`) or below (`col`) the pane showing `target`. */
export function splitPane(layout: TermLayout, target: string, sessionId: string, dir: "row" | "col"): TermLayout {
  return mapTabs(layout, (root) => mapLeaf(root, target, (leaf) => ({ kind: "split", dir, ratio: 0.5, a: leaf, b: { kind: "leaf", id: sessionId } })));
}

export function removePane(layout: TermLayout, sessionId: string): TermLayout {
  return mapTabs(layout, (root) => mapLeaf(root, sessionId, () => null));
}

export function replacePane(layout: TermLayout, from: string, to: string): TermLayout {
  return mapTabs(layout, (root) => mapLeaf(root, from, () => ({ kind: "leaf", id: to })));
}

/** Set the ratio of the split reached by following `path` ("a"/"b" steps) from a tab's root. */
export function setRatio(layout: TermLayout, tabId: string, path: string, ratio: number): TermLayout {
  const walk = (node: PaneNode, rest: string): PaneNode => {
    if (node.kind === "leaf") return node;
    if (rest === "") return { ...node, ratio: clampRatio(ratio) };
    return rest[0] === "a" ? { ...node, a: walk(node.a, rest.slice(1)) } : { ...node, b: walk(node.b, rest.slice(1)) };
  };
  return { ...layout, tabs: layout.tabs.map((t) => (t.id === tabId ? { ...t, root: walk(t.root, path) } : t)) };
}

export const tabOf = (layout: TermLayout, sessionId: string) => layout.tabs.find((t) => leaves(t.root).includes(sessionId));

function validNode(node: unknown): node is PaneNode {
  if (!node || typeof node !== "object") return false;
  const n = node as Record<string, unknown>;
  if (n.kind === "leaf") return typeof n.id === "string";
  return n.kind === "split" && (n.dir === "row" || n.dir === "col") && typeof n.ratio === "number" && validNode(n.a) && validNode(n.b);
}

/** Parse a saved layout, ignoring anything malformed. */
export function parseLayout(json: string | null): TermLayout {
  try {
    const raw = JSON.parse(json ?? "null") as { tabs?: unknown; active?: unknown } | null;
    if (!raw || !Array.isArray(raw.tabs)) return emptyLayout();
    const tabs = raw.tabs.filter((t): t is TermTab => !!t && typeof t.id === "string" && validNode(t.root));
    return { tabs, active: typeof raw.active === "string" ? raw.active : null };
  } catch {
    return emptyLayout();
  }
}

function prune(node: PaneNode, keep: (id: string) => boolean): PaneNode | null {
  if (node.kind === "leaf") return keep(node.id) ? node : null;
  const a = prune(node.a, keep);
  const b = prune(node.b, keep);
  if (!a) return b;
  if (!b) return a;
  return { ...node, a, b };
}

/** Make a layout match the sessions that exist: drop panes of deleted sessions (and duplicates),
 *  and give every session that has no pane a tab of its own. */
export function reconcile(layout: TermLayout, sessionIds: string[]): TermLayout {
  const seen = new Set<string>();
  const keep = (id: string) => {
    if (!sessionIds.includes(id) || seen.has(id)) return false;
    seen.add(id);
    return true;
  };
  const tabs = layout.tabs.flatMap((t) => {
    const root = prune(t.root, keep);
    return root ? [{ ...t, root }] : [];
  });
  for (const id of sessionIds) if (!seen.has(id)) tabs.push({ id: newId(), root: { kind: "leaf", id } });
  const active = tabs.some((t) => t.id === layout.active) ? layout.active : (tabs[0]?.id ?? null);
  return { tabs, active };
}

/** A box in fractions (0..1) of the tab area. */
export interface Rect { x: number; y: number; w: number; h: number }
export interface Divider { path: string; dir: "row" | "col"; box: Rect; ratio: number }

/** Where every pane and every split divider of a tree goes. Panes are laid out from this rather
 *  than nested in the tree, so splitting or closing one never re-creates its neighbours. */
export function place(node: PaneNode, box: Rect = { x: 0, y: 0, w: 1, h: 1 }, path = ""): { panes: Map<string, Rect>; dividers: Divider[] } {
  if (node.kind === "leaf") return { panes: new Map([[node.id, box]]), dividers: [] };
  const row = node.dir === "row";
  const first: Rect = row ? { ...box, w: box.w * node.ratio } : { ...box, h: box.h * node.ratio };
  const second: Rect = row
    ? { ...box, x: box.x + first.w, w: box.w - first.w }
    : { ...box, y: box.y + first.h, h: box.h - first.h };
  const a = place(node.a, first, `${path}a`);
  const b = place(node.b, second, `${path}b`);
  return {
    panes: new Map([...a.panes, ...b.panes]),
    dividers: [{ path, dir: node.dir, box, ratio: node.ratio }, ...a.dividers, ...b.dividers],
  };
}
