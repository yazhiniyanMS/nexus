// Frontend unit tests for the Model Lab's pure logic. Run with `npm test` (Node's built-in
// test runner; Node loads the TypeScript sources directly, so there is no build step).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  UNKNOWN, describeParams, emptyParams, filterModels, fmtBytes, fmtInt, fmtMs, fmtParams, fmtStat, fmtTps, hostOf,
  newRequestId, normalizeParams, PARAM_SPECS, paramsFor, parseParam, percent, ratio, sortModels,
} from "../src/lib/lab.ts";

test("unknown measurements are shown as unknown, never as zero", () => {
  for (const fmt of [fmtMs, fmtTps, fmtInt, fmtBytes]) {
    assert.equal(fmt(null), UNKNOWN);
    assert.equal(fmt(undefined), UNKNOWN);
    assert.equal(fmt(Number.NaN), UNKNOWN);
  }
  assert.equal(fmtParams(null, null), UNKNOWN);
  assert.equal(fmtParams(0, ""), UNKNOWN);
  assert.equal(fmtStat(null, fmtMs), UNKNOWN);
  // A real zero is still a value.
  assert.equal(fmtInt(0), "0");
  assert.equal(fmtTps(0), "0.0 tok/s");
});

test("durations read naturally at every scale", () => {
  assert.equal(fmtMs(0.4), "<1 ms");
  assert.equal(fmtMs(849.6), "850 ms");
  assert.equal(fmtMs(1353.4), "1.35 s");
  assert.equal(fmtMs(32838), "32.8 s");
  assert.equal(fmtMs(125_000), "2m 05s");
});

test("sizes, speeds and counts are formatted", () => {
  assert.equal(fmtBytes(512), "512 B");
  assert.equal(fmtBytes(1185376), "1.1 MB");
  assert.equal(fmtBytes(18174721847), "16.9 GB");
  assert.equal(fmtTps(32.5203), "32.5 tok/s");
  assert.equal(fmtTps(250.4), "250 tok/s");
  assert.equal(fmtInt(262144), "262,144");
  assert.equal(fmtParams(27781081984, null), "28B");
  assert.equal(fmtParams(7_600_000_000, null), "7.6B");
  assert.equal(fmtParams(15_000_000, null), "15.0M");
  assert.equal(fmtParams(260_000, null), "260K");
  assert.equal(fmtParams(27781081984, "27.8B"), "27.8B", "the runtime's own wording wins");
});

test("progress is only a percentage when the total is known", () => {
  assert.equal(percent(50, 200), 25);
  assert.equal(percent(250, 200), 100);
  assert.equal(percent(10, null), null);
  assert.equal(percent(10, 0), null);
});

test("the host a request goes to is extracted from the address", () => {
  assert.equal(hostOf("https://api.anthropic.com/v1"), "api.anthropic.com");
  assert.equal(hostOf("http://localhost:11434"), "localhost:11434");
  assert.equal(hostOf("http://user:pw@host:8080/x"), "host:8080");
  assert.equal(hostOf("not a url"), "not a url");
});

test("generation settings: unset stays unset, and only supported ones are kept", () => {
  const p = { ...emptyParams(), temperature: 0, top_k: 40, think: false, max_tokens: 64 };
  assert.deepEqual(paramsFor(p, ["temperature", "max_tokens"]), { ...emptyParams(), temperature: 0, max_tokens: 64 });
  assert.deepEqual(paramsFor(p, []), emptyParams());
  assert.deepEqual(normalizeParams({ temperature: 0.2 }), { ...emptyParams(), temperature: 0.2 });
  assert.deepEqual(normalizeParams(null), emptyParams());
  assert.equal(describeParams(emptyParams()), "runtime defaults");
  assert.equal(describeParams(p), "temperature 0 · top-k 40 · max output tokens 64 · thinking off");
});

test("setting fields are validated; empty means not set", () => {
  const spec = (key) => PARAM_SPECS.find((s) => s.key === key);
  assert.deepEqual(parseParam(spec("temperature"), "  "), { value: null });
  assert.deepEqual(parseParam(spec("temperature"), "0"), { value: 0 });
  assert.deepEqual(parseParam(spec("temperature"), "0.7"), { value: 0.7 });
  assert.match(parseParam(spec("temperature"), "3").error, /between 0 and 2/);
  assert.match(parseParam(spec("temperature"), "warm").error, /must be a number/);
  assert.match(parseParam(spec("max_tokens"), "12.5").error, /whole number/);
  assert.match(parseParam(spec("max_tokens"), "0").error, /between 1 and/);
  assert.deepEqual(parseParam(spec("seed"), "42"), { value: 42 });
});

const model = (over) => ({
  key: over.model_id, model_id: "m", kind: "ollama", provider_name: "Ollama", status: "installed", favorite: false, local: true,
  ...over,
  meta: { display_name: null, family: null, quantization: null, size_bytes: null, parameter_count: null, context_length: null, capabilities: [], ...(over.meta ?? {}) },
});

const LIBRARY = [
  model({ model_id: "qwen3.8:27b", meta: { family: "qwen3_5", quantization: "nvfp4", size_bytes: 18e9, parameter_count: 27.8e9, capabilities: ["vision", "tools"] }, favorite: true }),
  model({ model_id: "tiny.gguf", kind: "file", provider_name: null, status: "file", meta: { quantization: "Q4_K_M", size_bytes: 1.1e6, parameter_count: 260_000 } }),
  model({ model_id: "claude-opus-5-5", kind: "anthropic", provider_name: "Anthropic", status: "remote", local: false, meta: { display_name: "Claude Opus 5.5", context_length: 1_000_000 } }),
  model({ model_id: "Alpha:1", meta: {} }),
];
const all = { query: "", source: "all", status: "all", favoritesOnly: false };
const ids = (list) => list.map((m) => m.model_id);

test("the library is searched across names, families, quantization and capabilities", () => {
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, query: "QWEN vision" })), ["qwen3.8:27b"]);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, query: "q4_k_m" })), ["tiny.gguf"]);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, query: "opus" })), ["claude-opus-5-5"], "display names are searched");
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, query: "nothing-matches" })), []);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, favoritesOnly: true })), ["qwen3.8:27b"]);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, status: "remote" })), ["claude-opus-5-5"]);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, source: "Model folder" })), ["tiny.gguf"]);
  assert.deepEqual(ids(filterModels(LIBRARY, { ...all, source: "Ollama" })), ["qwen3.8:27b", "Alpha:1"]);
});

test("sorting keeps unknown sizes last in both directions and does not mutate", () => {
  const before = ids(LIBRARY);
  assert.deepEqual(ids(sortModels(LIBRARY, "size", true)), ["qwen3.8:27b", "tiny.gguf", "Alpha:1", "claude-opus-5-5"]);
  assert.deepEqual(ids(sortModels(LIBRARY, "size", false)), ["tiny.gguf", "qwen3.8:27b", "Alpha:1", "claude-opus-5-5"]);
  assert.deepEqual(ids(sortModels(LIBRARY, "name", false)), ["Alpha:1", "claude-opus-5-5", "qwen3.8:27b", "tiny.gguf"]);
  assert.deepEqual(ids(sortModels(LIBRARY, "provider", false)), ["claude-opus-5-5", "tiny.gguf", "Alpha:1", "qwen3.8:27b"]);
  assert.deepEqual(ids(sortModels(LIBRARY, "status", false)), ["Alpha:1", "qwen3.8:27b", "tiny.gguf", "claude-opus-5-5"]);
  assert.deepEqual(ids(sortModels(LIBRARY, "context", true))[0], "claude-opus-5-5");
  assert.deepEqual(ids(LIBRARY), before);
});

test("benchmark statistics are summarised with their spread and run count", () => {
  const stat = { n: 6, mean: 1200, median: 1100, min: 900, max: 1800, stddev: 300 };
  assert.equal(fmtStat(stat, fmtMs), "1.10 s (900 ms–1.80 s, 6 runs)");
  assert.equal(fmtStat({ ...stat, n: 1 }, fmtMs), "1.10 s (1 run)");
  assert.equal(fmtStat({ ...stat, n: 0 }, fmtMs), UNKNOWN);
  assert.equal(ratio(40, 20), 2);
  assert.equal(ratio(40, null), null);
  assert.equal(ratio(0, 20), null);
});

test("request ids are unique and in the form the core accepts", () => {
  const a = newRequestId();
  const b = newRequestId();
  assert.notEqual(a, b);
  for (const id of [a, b]) {
    assert.match(id, /^[A-Za-z0-9-]{8,64}$/);
  }
});
