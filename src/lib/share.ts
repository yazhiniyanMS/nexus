// Text a user can choose to share: a dependency map as a Mermaid diagram (GitHub renders it in
// READMEs, issues and pull requests) and measured results as a Markdown table. Nothing here
// sends anything anywhere; the caller copies or saves the text only when the user asks.
// Kept free of runtime imports so the Node test runner can load it directly.
import type { Graph } from "./api";

export const MADE_WITH = "Made with NEXUS (https://github.com/yazhiniyanMS/nexus)";

/** Mermaid labels are quoted; quotes and the characters Mermaid treats specially become entities. */
function mermaidLabel(s: string): string {
  return `"${s.replace(/"/g, "#quot;").replace(/</g, "#lt;").replace(/>/g, "#gt;")}"`;
}

/**
 * A fenced Mermaid flowchart of a dependency graph: module (or file) names and the imports between
 * them, with modules in a cycle highlighted. Contains names only, never source code.
 */
export function mermaidFromGraph(graph: Graph, title: string): string {
  const ids = new Map(graph.nodes.map((n, i) => [n.id, `n${i}`]));
  const lines = ["```mermaid", `%% ${title.replace(/[\r\n]+/g, " ")} — ${MADE_WITH}. Names only, no source code.`, "flowchart LR"];
  for (const n of graph.nodes) lines.push(`  ${ids.get(n.id)}[${mermaidLabel(n.label)}]`);
  for (const e of graph.edges) {
    const from = ids.get(e.from);
    const to = ids.get(e.to);
    if (from && to) lines.push(`  ${from} --> ${to}`);
  }
  const inCycle = graph.nodes.filter((n) => n.in_cycle).map((n) => ids.get(n.id));
  if (inCycle.length) {
    lines.push("  classDef cycle stroke:#e5484d,stroke-width:2px");
    lines.push(`  class ${inCycle.join(",")} cycle`);
  }
  lines.push("```");
  if (graph.truncated) lines.push("", "_Only the most connected nodes are shown._");
  return lines.join("\n");
}

export interface ShareTable {
  title: string;
  /** Lines shown under the title: machine, settings, date. */
  context: string[];
  columns: string[];
  rows: { label: string; cells: string[] }[];
  note?: string;
}

const cell = (s: string) => s.replace(/\|/g, "\\|").replace(/[\r\n]+/g, " ").trim() || "—";

/** A Markdown table with a short context block and a credit line. */
export function markdownTable(t: ShareTable): string {
  const out = [`### ${cell(t.title)}`, ""];
  for (const c of t.context) out.push(`- ${c}`);
  if (t.context.length) out.push("");
  out.push(`| | ${t.columns.map(cell).join(" | ")} |`);
  out.push(`| --- |${t.columns.map(() => " --- |").join("")}`);
  for (const r of t.rows) out.push(`| ${cell(r.label)} | ${t.columns.map((_, i) => cell(r.cells[i] ?? "")).join(" | ")} |`);
  out.push("");
  if (t.note) out.push(t.note, "");
  out.push(`_${MADE_WITH}._`);
  return out.join("\n");
}
