// Frontend unit tests for the Terminal's pane layout model.
import assert from "node:assert/strict";
import { test } from "node:test";
import { addTab, emptyLayout, leaves, parseLayout, place, reconcile, removePane, replacePane, setRatio, splitPane, tabOf } from "../src/lib/termLayout.ts";

const ids = (layout) => layout.tabs.map((t) => leaves(t.root));

test("tabs and splits are built up and torn down without losing panes", () => {
  let l = addTab(emptyLayout(), "a");
  l = splitPane(l, "a", "b", "row");
  l = splitPane(l, "b", "c", "col");
  l = addTab(l, "d");
  assert.deepEqual(ids(l), [["a", "b", "c"], ["d"]]);
  assert.equal(l.active, l.tabs[1].id, "a new tab becomes the active one");
  assert.equal(tabOf(l, "c").id, l.tabs[0].id);

  l = removePane(l, "b");
  assert.deepEqual(ids(l), [["a", "c"], ["d"]], "a split with one side gone collapses");
  l = replacePane(l, "c", "c2");
  assert.deepEqual(ids(l), [["a", "c2"], ["d"]]);
  l = removePane(l, "d");
  assert.deepEqual(ids(l), [["a", "c2"]]);
  assert.equal(l.active, l.tabs[0].id, "closing the active tab activates a remaining one");
  l = removePane(removePane(l, "a"), "c2");
  assert.deepEqual(l, { tabs: [], active: null });
});

test("split ratios are clamped and addressed by path", () => {
  let l = splitPane(splitPane(addTab(emptyLayout(), "a"), "a", "b", "row"), "b", "c", "col");
  const tab = l.tabs[0].id;
  l = setRatio(l, tab, "", 0.3);
  l = setRatio(l, tab, "b", 5);
  assert.equal(l.tabs[0].root.ratio, 0.3);
  assert.equal(l.tabs[0].root.b.ratio, 0.9, "ratios stay within 10%..90%");
  const { panes, dividers } = place(l.tabs[0].root);
  const round = (r) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, Math.round(v * 1000) / 1000]));
  assert.deepEqual(round(panes.get("a")), { x: 0, y: 0, w: 0.3, h: 1 });
  assert.deepEqual(round(panes.get("b")), { x: 0.3, y: 0, w: 0.7, h: 0.9 });
  assert.deepEqual(round(panes.get("c")), { x: 0.3, y: 0.9, w: 0.7, h: 0.1 });
  assert.deepEqual(dividers.map((d) => [d.path, d.dir]), [["", "row"], ["b", "col"]]);
});

test("a saved layout is reconciled with the sessions that still exist", () => {
  const saved = splitPane(splitPane(addTab(emptyLayout(), "a"), "a", "gone", "row"), "a", "b", "col");
  const l = reconcile(saved, ["a", "b", "new"]);
  assert.deepEqual(ids(l), [["a", "b"], ["new"]], "deleted sessions leave, unknown sessions get their own tab");
  assert.equal(l.active, saved.active);
  // A pane may show a session only once.
  const dup = { tabs: [{ id: "t1", root: { kind: "leaf", id: "a" } }, { id: "t2", root: { kind: "split", dir: "row", ratio: 0.5, a: { kind: "leaf", id: "a" }, b: { kind: "leaf", id: "b" } } }], active: "t2" };
  assert.deepEqual(ids(reconcile(dup, ["a", "b"])), [["a"], ["b"]]);
  assert.deepEqual(reconcile(emptyLayout(), []), { tabs: [], active: null });
});

test("malformed saved layouts are ignored rather than trusted", () => {
  for (const bad of [null, "", "not json", "{}", '{"tabs": 5}', '{"tabs":[{"id":1,"root":{"kind":"leaf","id":"a"}}]}', '{"tabs":[{"id":"t","root":{"kind":"split","dir":"diagonal","ratio":0.5,"a":{"kind":"leaf","id":"a"},"b":{"kind":"leaf","id":"b"}}}]}']) {
    assert.deepEqual(parseLayout(bad), { tabs: [], active: null }, String(bad));
  }
  const good = JSON.stringify(splitPane(addTab(emptyLayout(), "a"), "a", "b", "row"));
  assert.deepEqual(ids(parseLayout(good)), [["a", "b"]]);
});
