# Blog post: mapping a codebase without AI

> Draft for the maintainer to publish (dev.to, Hashnode or your own blog). Every detail is from the NEXUS source; file references are real. This is the second technical post; the first is [TECHNICAL_ARTICLE.md](TECHNICAL_ARTICLE.md).

---

# Mapping a codebase without AI: what a few hundred lines of regex get right, and where they fail

When I open a repository I didn't write, the first question isn't "what does this function do?" — it's "what are the parts, and which part depends on which?" You don't need a language model for that. You need the files, their imports, and a graph. This post walks through how [NEXUS](https://github.com/yazhiniyanMS/nexus), an open-source desktop app, builds its architecture map locally in a few seconds, and the places where the simple approach is honestly wrong.

## 1. Walk only what the project considers source

The walker is the [`ignore`](https://crates.io/crates/ignore) crate — the one ripgrep uses — so `.gitignore` is respected for free, plus a short list of directories that are always generated (`node_modules`, `target` and friends). Hidden files are included (`.github` matters), symlinks are not followed, and binary or oversized files are skipped by looking for a NUL byte in the first 8 KB (`src-tauri/src/projects.rs`). The index stops at 20,000 source files.

That one decision — respect what the project says is not source — removes most of the noise before any parsing happens.

## 2. Extract definitions and imports, line by line

`src-tauri/src/intel/extract.rs` has one function per language family: Rust, TypeScript/JavaScript, Python and Go. Each is a handful of regular expressions applied line by line. For TypeScript, a definition is a line that looks like `export function x`, `class X`, `interface X`, `type X`, `enum X`, or `const x = (...) =>`. Imports are `import … from '…'`, `export … from '…'`, `require('…')` and `import('…')`.

Rust needed two special cases:

- `mod engine;` is not a definition — it's a pointer to another file — so it's recorded as an import.
- `use` statements span lines and nest braces, so they're matched against the whole text, and `use crate::{a, b::C}` is expanded into `crate::a` and `crate::b::C`.

Everything sits behind one function, `extract(lang, text)`, so a real parser (tree-sitter is the obvious candidate) can replace a language without touching storage, commands or the interface.

## 3. Resolve imports to files — the part that actually matters

An import string is useless until you know which file it means. `src-tauri/src/intel/resolve.rs` turns each one into *a file inside the project*, *an external package*, or *nothing*:

- **TypeScript/JavaScript:** relative paths are joined to the importing file's folder and tried with each extension and as `index.*`; `./x.js` in TypeScript means `./x.ts`; `@/` means `src/`. Anything else is a package (`@scope/name` counts as one package).
- **Rust:** `crate::` walks up to the nearest folder with `lib.rs` or `main.rs` and follows module files from there; `super::` and `self::` work relative to the file; `use sibling::Item` is tried as a child module *before* it's treated as an external crate; `std`, `core` and `alloc` are ignored.
- **Python:** relative imports with dots, a list of standard-library modules to ignore, everything else a package.
- **Go:** the module path from `go.mod` decides what's internal.

Imports that resolve to the importing file itself are dropped; duplicates per file are counted once.

## 4. From files to modules to a picture

A "module" is just a file's first *n* folders (`src-tauri/src/intel/graph.rs`): at depth 2, `src/views/lab/Compare.tsx` belongs to `src/views`. File-level edges are summed into module-level edges, which become the arrow thickness.

Cycles are strongly connected components with more than one node — Tarjan's algorithm, written iteratively so a deep graph can't overflow the stack. NEXUS's own map at three levels has two. One is in the interface: `src`, `src/components` and the `src/views` folders import each other (the app shell imports the views and components, and four components import the `Workspace` type back from `App.tsx` — a type-only import, which is exactly the kind of edge a line-based index can't tell apart from a real one). The other is the Rust core: `lib.rs` declares every module with `mod`, and the modules reach back into the crate root with `crate::`. The second is just how Rust crates are laid out; the first is a real, if mild, tangle. I wouldn't have seen either by reading files.

Picking *n* turned out to matter more than any of the parsing. Most of NEXUS's code sits in `src-tauri/src/…`, so at two levels the whole Rust core is one circle: 9 modules and 10 dependencies. At three levels it's 16 modules and 29 dependencies. The map now tries depths 1 to 3 and opens at the one with the most dependencies that still has at most 36 modules — small enough to read.

## 5. Sharing the shape without sharing the code

The same graph feeds two things you can share:

- **A report:** one self-contained HTML file with the graph, the most-imported files, dependencies and findings. It has a `default-src 'none'` Content-Security-Policy, loads nothing, and a test plants recognisable source text in a project and checks that none of it appears in the report.
- **A Mermaid diagram:** "Copy as Mermaid" puts the map on your clipboard as a ```` ```mermaid ```` block, which GitHub renders as a picture in READMEs, issues and pull requests. Module names and arrows, nothing else.

## Where this is wrong

Regexes see text, not syntax, and it shows:

- `require('x')` inside a comment or a string counts as an import.
- TypeScript path aliases other than `@/` (from `tsconfig.json` `paths`) aren't resolved, so those imports disappear from the map.
- TypeScript class methods aren't indexed as symbols, and an arrow function whose `=>` is on the next line is missed.
- Only five languages are understood. Java, C#, Kotlin, C and C++ projects get an empty map (search and the other analysis still work).

For a map, these errors mostly cost a missing arrow, which is a better failure than an invented one. For symbol search and references they matter more, which is why the extractor sits behind one function waiting for a parser.

## Try it

NEXUS is MIT-licensed and runs on macOS, Windows and Linux: https://github.com/yazhiniyanMS/nexus. The map and the report need no account, no key and no model. If you want to help, Java support in `extract.rs` and `resolve.rs` is a self-contained first contribution.
