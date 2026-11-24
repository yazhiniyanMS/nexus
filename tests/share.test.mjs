import assert from "node:assert/strict";
import test from "node:test";
import { markdownTable, mermaidFromGraph } from "../src/lib/share.ts";

const node = (id, in_cycle = false) => ({ id, label: id, kind: "module", files: 1, lines: 10, fan_in: 0, fan_out: 0, in_cycle });

test("the map is shared as a Mermaid diagram of names and imports, with cycles marked", () => {
  const graph = {
    nodes: [node("src", true), node("src/views", true), node('lib "quoted" <x>')],
    edges: [{ from: "src", to: "src/views", weight: 2 }, { from: "src/views", to: "src", weight: 1 }, { from: "src", to: "gone", weight: 1 }],
    cycles: [["src", "src/views"]],
    truncated: true,
  };
  const md = mermaidFromGraph(graph, "Architecture map,\nthree levels");
  const lines = md.split("\n");
  assert.equal(lines[0], "```mermaid");
  assert.ok(lines[1].startsWith("%% Architecture map, three levels — Made with NEXUS"));
  assert.equal(lines[2], "flowchart LR");
  assert.ok(md.includes('n0["src"]') && md.includes('n1["src/views"]'));
  assert.ok(md.includes('n2["lib #quot;quoted#quot; #lt;x#gt;"]'), "labels are escaped");
  assert.ok(md.includes("n0 --> n1") && md.includes("n1 --> n0"));
  assert.ok(!md.includes("gone"), "edges to unknown nodes are dropped");
  assert.ok(md.includes("class n0,n1 cycle"));
  assert.ok(md.includes("most connected nodes"));
});

test("benchmark results are shared as a Markdown table with the context needed to read them", () => {
  const md = markdownTable({
    title: "Local model benchmark",
    context: ["Machine: macOS 27 · arm64", "Measured 2026-11-25."],
    columns: ["model-a (local)", "model|b (local)"],
    rows: [{ label: "Generation speed", cells: ["42.0 tok/s", ""] }],
    note: "Speed only.",
  });
  assert.ok(md.startsWith("### Local model benchmark\n\n- Machine: macOS 27 · arm64\n- Measured 2026-11-25.\n"));
  assert.ok(md.includes("| | model-a (local) | model\\|b (local) |"), "pipes in names are escaped");
  assert.ok(md.includes("| --- | --- | --- |"));
  assert.ok(md.includes("| Generation speed | 42.0 tok/s | — |"), "empty cells show a dash");
  assert.ok(md.trimEnd().endsWith("_Made with NEXUS (https://github.com/yazhiniyanMS/nexus)._"));
});
