import { useEffect, useMemo, useRef, useState } from "react";
import type { Graph, GraphNode } from "../lib/api";

type Point = { x: number; y: number };
const W = 1000;
const H = 700;

const radius = (n: GraphNode) => (n.kind === "external" ? 7 : Math.min(34, 9 + Math.sqrt(n.lines) / 3.5));

/** Deterministic force-directed layout (Fruchterman–Reingold). Small graphs only, so it runs synchronously. */
function layout(graph: Graph): Map<string, Point> {
  const n = graph.nodes.length;
  const pos = new Map<string, Point>();
  graph.nodes.forEach((node, i) => {
    const a = (i / Math.max(1, n)) * Math.PI * 2;
    pos.set(node.id, { x: W / 2 + Math.cos(a) * W * 0.3, y: H / 2 + Math.sin(a) * H * 0.3 });
  });
  if (n < 2) return pos;
  const k = Math.sqrt((W * H) / n) * 0.75;
  const ids = graph.nodes.map((x) => x.id);
  for (let iter = 0; iter < 260; iter++) {
    const temp = 60 * (1 - iter / 260) + 1;
    const disp = new Map<string, Point>(ids.map((id) => [id, { x: 0, y: 0 }]));
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const a = pos.get(ids[i])!, b = pos.get(ids[j])!;
        let dx = a.x - b.x, dy = a.y - b.y;
        const d = Math.max(0.01, Math.hypot(dx, dy));
        const f = (k * k) / d;
        dx = (dx / d) * f; dy = (dy / d) * f;
        disp.get(ids[i])!.x += dx; disp.get(ids[i])!.y += dy;
        disp.get(ids[j])!.x -= dx; disp.get(ids[j])!.y -= dy;
      }
    }
    for (const e of graph.edges) {
      const a = pos.get(e.from), b = pos.get(e.to);
      if (!a || !b) continue;
      const dx = a.x - b.x, dy = a.y - b.y;
      const d = Math.max(0.01, Math.hypot(dx, dy));
      const f = (d * d) / k;
      disp.get(e.from)!.x -= (dx / d) * f; disp.get(e.from)!.y -= (dy / d) * f;
      disp.get(e.to)!.x += (dx / d) * f; disp.get(e.to)!.y += (dy / d) * f;
    }
    for (const id of ids) {
      const p = pos.get(id)!, d = disp.get(id)!;
      d.x += (W / 2 - p.x) * 0.02 * k * 0.1; d.y += (H / 2 - p.y) * 0.02 * k * 0.1;
      const len = Math.max(0.01, Math.hypot(d.x, d.y));
      p.x = Math.min(W - 40, Math.max(40, p.x + (d.x / len) * Math.min(len, temp)));
      p.y = Math.min(H - 40, Math.max(40, p.y + (d.y / len) * Math.min(len, temp)));
    }
  }
  return pos;
}

export function GraphView({ graph, selected, onSelect }: { graph: Graph; selected: string | null; onSelect: (id: string | null) => void }) {
  const initial = useMemo(() => layout(graph), [graph]);
  const [pos, setPos] = useState(initial);
  const [view, setView] = useState({ x: 0, y: 0, k: 1 });
  const [hover, setHover] = useState<string | null>(null);
  const drag = useRef<{ id: string | null; sx: number; sy: number; ox: number; oy: number; moved: boolean } | null>(null);
  const svg = useRef<SVGSVGElement>(null);

  useEffect(() => {
    setPos(initial);
    setView({ x: 0, y: 0, k: 1 });
  }, [initial]);

  // Screen pixels -> graph units, accounting for the SVG's own scaling.
  const scale = () => (svg.current ? W / svg.current.getBoundingClientRect().width : 1);
  const focus = hover ?? selected;
  const linked = useMemo(() => {
    const set = new Set<string>();
    if (focus) for (const e of graph.edges) if (e.from === focus || e.to === focus) { set.add(e.from); set.add(e.to); }
    return set;
  }, [focus, graph.edges]);

  return (
    <svg
      ref={svg}
      viewBox={`0 0 ${W} ${H}`}
      className="h-full w-full cursor-grab touch-none active:cursor-grabbing"
      data-testid="graph"
      onWheel={(e) => {
        const k = Math.min(4, Math.max(0.4, view.k * (e.deltaY < 0 ? 1.12 : 0.89)));
        setView((v) => ({ ...v, k }));
      }}
      onPointerDown={(e) => {
        const id = (e.target as Element).closest("[data-node]")?.getAttribute("data-node") ?? null;
        const p = id ? pos.get(id)! : view;
        drag.current = { id, sx: e.clientX, sy: e.clientY, ox: p.x, oy: p.y, moved: false };
        (e.currentTarget as Element).setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const dx = (e.clientX - d.sx) * scale(), dy = (e.clientY - d.sy) * scale();
        if (Math.abs(dx) + Math.abs(dy) > 3) d.moved = true;
        if (d.id) setPos((m) => new Map(m).set(d.id!, { x: d.ox + dx / view.k, y: d.oy + dy / view.k }));
        else setView((v) => ({ ...v, x: d.ox + dx, y: d.oy + dy }));
      }}
      onPointerUp={() => {
        const d = drag.current;
        drag.current = null;
        if (d && !d.moved) onSelect(d.id);
      }}
    >
      <defs>
        <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill="#5b5b66" />
        </marker>
        <marker id="arrow-hi" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill="#86dfc4" />
        </marker>
      </defs>
      <g transform={`translate(${view.x + (W / 2) * (1 - view.k)} ${view.y + (H / 2) * (1 - view.k)}) scale(${view.k})`}>
        {graph.edges.map((e) => {
          const a = pos.get(e.from), b = pos.get(e.to);
          const na = graph.nodes.find((n) => n.id === e.from), nb = graph.nodes.find((n) => n.id === e.to);
          if (!a || !b || !na || !nb) return null;
          const d = Math.max(0.01, Math.hypot(b.x - a.x, b.y - a.y));
          const ux = (b.x - a.x) / d, uy = (b.y - a.y) / d;
          const hi = focus !== null && (e.from === focus || e.to === focus);
          return (
            <line
              key={`${e.from}>${e.to}`}
              x1={a.x + ux * radius(na)} y1={a.y + uy * radius(na)}
              x2={b.x - ux * (radius(nb) + 3)} y2={b.y - uy * (radius(nb) + 3)}
              stroke={hi ? "#86dfc4" : "#3a3a42"}
              strokeOpacity={focus && !hi ? 0.25 : 0.9}
              strokeWidth={Math.min(4, 0.8 + Math.log2(1 + e.weight) * 0.7)}
              markerEnd={hi ? "url(#arrow-hi)" : "url(#arrow)"}
            />
          );
        })}
        {graph.nodes.map((n) => {
          const p = pos.get(n.id);
          if (!p) return null;
          const dim = focus !== null && n.id !== focus && !linked.has(n.id);
          const fill = n.kind === "external" ? "#3f3f46" : n.kind === "file" ? "#b4a8f2" : "#86dfc4";
          return (
            <g
              key={n.id}
              data-node={n.id}
              transform={`translate(${p.x} ${p.y})`}
              opacity={dim ? 0.3 : 1}
              className="cursor-pointer"
              onPointerEnter={() => setHover(n.id)}
              onPointerLeave={() => setHover(null)}
            >
              <circle r={radius(n)} fill={fill} fillOpacity={n.kind === "external" ? 0.9 : 0.22} stroke={n.in_cycle ? "#f87171" : n.id === selected ? "#ffffff" : fill} strokeWidth={n.id === selected || n.in_cycle ? 2.5 : 1.2} />
              <text y={radius(n) + 13} textAnchor="middle" fontSize={12} fill={n.kind === "external" ? "#9a9aa3" : "#ededee"} className="pointer-events-none select-none">
                {n.label.length > 26 ? `…${n.label.slice(-25)}` : n.label}
              </text>
            </g>
          );
        })}
      </g>
    </svg>
  );
}
