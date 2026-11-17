# NEXUS 2.0 — Architecture audit, proposal and plan

Status: Milestone 1 (Project Intelligence) is implemented. Milestones 2–6 are planned, not built.

## 1. Audit of the current application

### What exists

| Layer | State |
| --- | --- |
| Shell | Tauri 2, one window, strict CSP, capabilities limited to `core:default` and `dialog:default`. The webview has no filesystem or shell plugin. |
| Backend | About 2,900 lines of Rust in one crate. One module per feature: `projects`, `search`, `git`, `tasks`, `ai`, `activity`, `db`. All webview access goes through typed `#[tauri::command]` functions registered in `lib.rs`. |
| State | `AppState { db, engine, http }`. SQLite behind a mutex, WAL mode, forward-only migrations keyed on `user_version`. |
| Frontend | About 2,100 lines of React. `lib/api.ts` is the only file that calls `invoke`. Views are switched by local state; a `Workspace` object carries the project list and current project. |
| Tests | 20 Rust tests (parsing, analysis, search, Git against a real repository, task engine end to end on a mock Tauri app). No frontend tests. A debug-only bridge (`e2e.rs`) drives the real window. |
| CI | One workflow building macOS (arm64, x64), Windows and Linux. It has never run. |

### What can be extended instead of duplicated

| Existing piece | Reused by |
| --- | --- |
| `projects::walker`, `read_text`, `rel` (gitignore-aware walking, binary detection) | Code index, test discovery |
| `projects::resolve_inside` (path confinement) | Every command that takes a project-relative path |
| `search::search_file` (streaming line scan) | Reference search, terminal output search |
| `tasks` engine (approval gate, process-group kill, streamed output, cancellation) | Agent tool execution, test runs, benchmark runs. It should become a general *job runner* in Milestone 2 rather than gain siblings. |
| `ai::complete`, keychain storage, `ai_usage` | Source-linked answers, agents, Model Lab benchmarks |
| `git::git` (single choke point for the Git CLI) | Branch graph, PR preparation, conflict assistance |
| `activity::log` | Per-agent logs, test history |
| `FilePreview` / Monaco viewer | Symbol results, graph drill-down, review workflow |

### Weaknesses found

1. **Migrations were forward-only.** Fixed in Milestone 1: each migration now has an `up` and a `down`, with a test that applies, reverts and re-applies all of them.
2. **Navigation does not scale.** Seven flat sidebar items already; six more modules would overcrowd it. Fixed in Milestone 1 with a command palette, a global project switcher, keyboard shortcuts and a file "peek" drawer that any screen can open.
3. **Views own their own project selection** (`ProjectPicker` inside Git). Replaced by the single global switcher.
4. **`tasks.rs` mixes three concerns**: persistence, the approval state machine, and process execution. Agents, tests and the terminal all need the third. Planned extraction in Milestone 2.
5. **Blocking Git calls run on the async runtime.** Acceptable for status-sized calls, wrong for large logs. Move to `spawn_blocking` in Milestone 5.
6. **No frontend tests and no typed contract check** between Rust structs and `api.ts`. Planned: generate the TypeScript types from Rust (`specta` or `ts-rs`) in Milestone 2, so a mismatch fails the build.
7. **Chat is not streamed and not stored.** Addressed with the agent run log in Milestone 2.

### Should the stack change?

No. Nothing in the audit gives a concrete reason to leave Tauri 2, React, Rust and SQLite. Two additions are justified and scoped:

- A pseudo-terminal crate (`portable-pty`) for Milestone 3, because pipes cannot run interactive programs.
- A real parser (`tree-sitter`) is the right long-term basis for symbols. Milestone 1 uses line-oriented extractors behind a small interface (`intel::extract`) so the parser can be swapped in without touching storage, commands or UI. This is a deliberate limitation, listed below.

## 2. Target architecture

```
React UI (views/*)            one file per module, lazy-loaded
   │  lib/api.ts              typed bridge, the only invoke() caller
   ▼
Tauri commands (lib.rs)       thin: validate, call a module, map errors
   ▼
Modules (independent)         projects  search  git  intel  tasks
                              agents    terminal    models*   tests*
   ▼
Shared services               db (migrations)   jobs* (runner, approval, cancel)
                              ai (providers, keychain)   activity
```
`*` = planned.

Rules that keep modules independent:

- A module may depend on shared services and on `projects` (for path resolution). It may not call another feature module's commands. Cross-module flows go through the UI or through a shared service.
- Every module owns its tables, prefixed with its name (`intel_*`, `agent_*`, `term_*`, `test_*`, `model_*`).
- Expensive work runs in `spawn_blocking` or a spawned task, reports progress with events named `nexus://<module>-<event>`, and never holds the database lock across an `.await`.
- Anything that executes a process goes through the job runner, which is the only place that enforces approval and cancellation.
- AI is always optional: each feature must return something useful with no provider configured.

### Permission model (agents and extensions)

Introduced in Milestone 2 and enforced in the job runner, not in the UI:

| Capability | Default | Notes |
| --- | --- | --- |
| `read_project` | allowed | Confined by `resolve_inside`. |
| `search` / `intel` | allowed | Read-only. |
| `run_command` | ask every time | Existing approval gate. Destructive patterns always ask, even if the agent has a standing grant. |
| `write_file` | ask every time | Shown as a diff before approval. |
| `git_write` | ask every time | Never includes history rewriting without a separate explicit prompt. |
| `network` | denied | Only the configured AI provider endpoint is reachable by the core. |

Grants are stored per agent and per project, are revocable, and every use is written to that agent's log.

### Credentials

Unchanged: keys live in the OS keychain (`keyring`), SQLite stores only a "has key" flag, keys never cross into the webview. Agents and the terminal never receive provider keys in their environment.

## 3. Milestones

Each milestone follows the same steps: audit, design, backend, UI, automated tests, verification in the real app, documented limits.

### Milestone 1 — Project Intelligence and navigation (implemented)

Backend (`src-tauri/src/intel/`):

| Feature | Implementation |
| --- | --- |
| Code index | Background indexing of Rust, TypeScript/JavaScript, Python and Go into `intel_files`, `intel_symbols`, `intel_imports`. Progress events. |
| Symbol search | Ranked name search (exact, prefix, substring) with kind filter. |
| Reference search | Whole-word scan of indexed files; definitions are marked using the index. |
| Dependency graph | File-level and module-level graphs from resolved imports, with external packages optional. |
| Architecture map | Module graph with size, fan-in, fan-out and dependency cycles (Tarjan SCC). |
| Cross-file context | For any file: its symbols, what it imports, what imports it. |
| Source-linked answers | Keyword retrieval of numbered snippets (works with no AI). With a provider, the answer cites those snippets and each citation opens the file at the line. |
| Architecture document | Deterministic Markdown (modules, Mermaid graph, entry points, hotspots, cycles, external dependencies), optional AI narrative. Saved through a native dialog opened by the core, so the webview cannot choose the path. |

UI: an **Intelligence** screen (Map, Symbols, Ask, Docs), a command palette (`Ctrl/Cmd+K`), a global project switcher, view shortcuts (`Ctrl/Cmd+1…8`) and a file peek drawer.

Known limits of Milestone 1:

- Symbols and imports come from line-oriented extractors, not a parser. Nested or macro-generated definitions can be missed; Java, C#, C/C++, Kotlin and Swift are not indexed yet.
- References are textual (whole-word), not semantic: same-named symbols in different scopes are not distinguished.
- Retrieval for answers is keyword-based, not embedding-based.
- Import resolution covers relative paths, `crate::`/`super::`/`mod`, Python packages and Go module paths. TypeScript path aliases other than `@/` → `src/` are not resolved.
- The index is rebuilt in full; there is no file watcher or incremental update.
- "Customisable workspace layout" arrived in Milestone 3 as the Terminal's saved split layout; other screens have fixed layouts.

### Milestone 2 — Agent Command Center (implemented)

#### Inspection

- `tasks.rs` already had the hard parts of safe execution: an approval gate, streamed output, process-tree kill and cancellation. Its process code is extracted into `jobs.rs` and shared; the task engine is otherwise unchanged and its tests still pass.
- `ai::complete` already speaks Anthropic, OpenAI-compatible and Ollama. Agents reach it through a small `ModelClient` trait, so the engine never touches providers or keys and tests can substitute a scripted or failing client.
- Milestone 1's index (`intel`) gives agents grounded, read-only context: retrieval for evidence, the module graph for structure.

#### Design

An agent task is a **run**: an outcome, a project, a provider and a model. A run is a persisted list of **steps**, each owned by one role, with explicit dependencies. One engine drives every run; agents are not processes.

| Step | Role | What it does | Needs approval |
| --- | --- | --- | --- |
| Research | Research | Retrieves source passages for the outcome, then summarises the evidence with citations. | No (read-only) |
| Plan | Architect | Reads the project structure and the research, reports architectural issues, proposes a plan, the files to change and test commands. | — |
| Plan approval | You | Shows the plan. Reject ends the run. | **Yes** |
| Implement | Developer | Writes complete new file contents into an isolated Git worktree. Reports modified files. | Covered by the plan approval; nothing reaches the project yet. |
| Discover tests | Testing | Finds test commands from the project's manifests; adds the Architect's suggestions, marked as AI-proposed. | No (read-only) |
| Run command (one step each) | Testing | Runs one test command in the worktree, streams output, reports the exit code. | **Yes, each one** |
| Review | Review | Reviews the exact diff against the outcome, the plan and the test results. | No (read-only) |
| Apply | You | Shows the exact diff. Approve applies it to the project; reject discards it. | **Yes** |

Roles and permissions (enforced in the engine by `roles::require`, not in the UI):

| Role | read project | write worktree | run commands (after approval) |
| --- | :-: | :-: | :-: |
| Architect | ✓ | | |
| Research | ✓ | | |
| Developer | ✓ | ✓ | |
| Testing | ✓ | | ✓ |
| Review | ✓ | | |

No role can write to the project itself. Only the Apply step does, and only after approval.

#### Safety rules

- **Isolation:** every run gets `git worktree add --detach` at the project's current commit, under the app's data directory. Uncommitted work in the project is never changed and never included in the workspace. The Research agent does read the project as it is on disk (the code index, plus the top-level README and CONTRIBUTING files, which the index does not cover).
- **Writes are confined:** paths from a model must be relative, may not contain `..` or touch `.git`, may not pass through a symlink, and are re-checked against the canonical worktree root. Limits: 40 files, 400 KB each.
- **Commands are never trusted:** every command, discovered or AI-proposed, is shown verbatim and waits for approval. Destructive-looking commands are flagged. Commands run in the worktree, with credential-like environment variables removed.
- **Applying is all-or-nothing:** the diff is checked with `git apply --check` against your working tree first. If it would collide with your own edits, nothing is applied and the run says so. NEXUS never commits, stages or pushes.
- **Credentials:** the engine only holds a provider id; keys stay in the OS keychain and are read inside `ai`.
- **Network:** NEXUS itself only talks to the provider URL you configured. Commands you approve are not network-sandboxed (not practical across three operating systems without containers); this is stated on the approval card.

#### Orchestration

- **Queue:** runs start as `queued`; at most two execute at once.
- **Dependencies:** a step runs only when every step it depends on is completed or skipped.
- **Retries:** model steps retry twice with backoff on provider or parse errors; every attempt is logged. Commands are never retried automatically.
- **Pause / cancel:** pause takes effect before the next step; cancel kills a running command's process tree and abandons an in-flight model call.
- **Recovery:** every state change is written to SQLite first. On restart, runs that were executing are marked `interrupted`; Resume continues from the first unfinished step using the worktree that is still on disk.
- **Trace:** `agent_events` records every model call, tool use, approval and error with its role and time.

Tables: `agent_runs`, `agent_steps`, `agent_events` (migration 3, reversible).

#### Limits

- Agents need a Git repository with at least one commit.
- The Developer returns whole files, not patches, and cannot delete files.
- Pause does not suspend a command that is already running.
- Approved commands are not network-sandboxed.
- Steps within a run execute one at a time, in dependency order.
- Generated API types (planned here originally) are deferred.

### Milestone 3 — Terminal (implemented)

Backend (`src-tauri/src/terminal.rs`), using `portable-pty` (openpty on Unix, ConPTY on Windows):

| Part | Implementation |
| --- | --- |
| Sessions | One login shell per session (`$SHELL`, falling back to the account's shell, then `/bin/sh`; `ComSpec` on Windows), started in the project root or a folder inside it (`resolve_inside`). At most 16 running at once. |
| Output | A reader thread appends raw output to `terminal/<id>.log` in the app data directory and emits `nexus://term-output` with a stream position. The file is cut back to about 1 MB when it passes 2 MB. Characters split across reads are reassembled. |
| Reload safety | The UI subscribes first, then loads the saved output; the stream position lets it drop anything it already has, so nothing is lost or shown twice. |
| End of a shell | A waiter thread records `exited` with the exit code, or no code when the user stopped it. On the next start, sessions still marked running become `ended` and keep their output. |
| Stopping | SIGHUP to every process in the terminal's session, then SIGKILL to whatever is left (jobs in their own process groups included); `taskkill /T` on Windows. Removing a project stops its shells. |
| Search | Case-insensitive search of the saved output of every terminal in a project, with escape sequences removed. |
| Layout | `term_layouts` stores one JSON document per project (tabs, each a tree of row/column splits with ratios). The core validates only that it is JSON and its size. |

Tables: `term_sessions`, `term_layouts` (migration 4, reversible). The planned `term_history` table was dropped: a pseudo-terminal carries keystrokes, not commands, so a command history could not be recorded reliably. Output search covers the need.

UI (`src/views/Terminal.tsx`, `src/lib/termLayout.ts`): `xterm.js` bundled locally, tabs, split right / split down, draggable dividers, rename, stop, restart, close (with confirmation while running), status and exit code, output search with jump-to-terminal. Terminals are kept mounted and positioned from the layout tree, so changing the layout or switching tabs never re-creates one. This is the customisable workspace layout promised in Milestone 1.

Safety:

- `term_write` is the only way into a shell and is called only by the Terminal screen for keystrokes and pastes. Agents, tasks and AI code have no terminal access; their commands still go through the approval gates in `jobs`/`tasks`/`agents`.
- A terminal is deliberately not sandboxed: it is the user's shell with the user's permissions. It starts inside the project but can `cd` anywhere, like any terminal.
- While saved output is replayed, input is not forwarded, so terminal status queries in old output cannot type answers into a live shell.
- The debug-only UI test bridge is not passed into shells (`NEXUS_E2E_PORT` is removed from their environment).

Tests (72 total): real `/bin/sh` sessions on a mock app cover start folder and confinement, input, resize, exit code, stopping foreground and background jobs, close, search, layout storage, restart recovery and project removal; unit tests cover split UTF-8, escape stripping and scrollback trimming. Verified in the real app, including a bundle with the production content security policy started the way Finder starts it.

Known limits: shells do not survive closing NEXUS; saved output is replayed at the current width; no per-pane font or theme settings; the pseudo-terminal tests are Unix-only and Windows (ConPTY) is untested.

### Milestone 4 — Model Lab (implemented, experimental)

Built in `src-tauri/src/lab/` (`runtime`, `registry`, `gguf`, `storage`, `download`, `infer`, `bench`, `hardware`) and `src/views/lab/`, with automated tests. The notes below are the original plan; a full write-up of the implementation has not been added yet.


1. Extend `ai::detect_local` into a discovery service (Ollama, LM Studio, llama.cpp server, vLLM, any OpenAI-compatible URL).
2. Tables: `model_configs` (temperature, context, system prompt presets), `model_benchmarks` (prompt set, tokens/s, time to first token, total time, date, machine).
3. Streaming completions (also unblocks streamed chat and agents).
4. Benchmarks run as cancellable jobs; charts of history per model.
5. Tests against a local stub server.

### Milestone 5 — Git Intelligence

1. Move Git calls to `spawn_blocking`; add `git log --graph` parsing for a branch lane view.
2. Side-by-side and word-level diffs in Monaco's diff editor.
3. PR preparation: branch vs base summary, commit list, changed files, generated description (deterministic, optional AI).
4. Review workflow: per-file "viewed" state and local comments stored in SQLite.
5. Conflict assistance: parse conflict markers, show ours/theirs/base, apply a chosen side to the working file only after confirmation.
6. Write operations (commit, branch, checkout) each behind explicit confirmation; nothing that discards work without a prompt naming what is lost.

### Milestone 6 — Testing Lab

1. Discovery per ecosystem (cargo, vitest/jest, pytest, go test) reusing manifest detection.
2. Runs go through the job runner; machine-readable output parsed into `test_runs` / `test_results`.
3. Coverage from lcov / cobertura files.
4. Flaky detection: a test that both passed and failed on the same commit.
5. History and trend views.

### Cross-platform release (public beta 0.1.0)

**Platform layer** (`src-tauri/src/platform/`). Feature modules contain no operating-system checks; they call these services.

| Service | Module | macOS / Linux | Windows |
| --- | --- | --- | --- |
| Shell discovery | `shell` | `$SHELL`, then the account database, then `/bin/sh`; login flag for zsh, bash, fish, ksh | PowerShell 7, Windows PowerShell, then `ComSpec` |
| Process management | `process` | `sh -c` in its own process group; `killpg`; terminals stopped by session (SIGHUP, then SIGKILL) | `cmd /D /S /C` with UTF-8 output; `taskkill /T` for trees |
| Terminal / PTY | `terminal.rs` via `portable-pty` | `openpty` | ConPTY |
| Paths | `paths` | `canonicalize` | `canonicalize` with the `\\?\` prefix removed |
| Command search path | `env` | Login shell's `PATH`, resolved once at start with a 4 s limit | The process `PATH` |
| Programs, Git discovery | `programs` | Search path | Search path with `.exe/.cmd/.bat/.com`, then Git for Windows install folders |
| Secure storage | `secrets` | Keychain / Secret Service | Credential Manager |
| Application directories | `dirs` | Tauri's app-data folder, or `NEXUS_DATA_DIR` | Same |
| Disk space | `disk` | `statvfs` | `GetDiskFreeSpaceExW` |
| Notifications | `notify` | Tauri notification plugin, only when the window is not focused | Same |
| File dialogs | Tauri dialog plugin, opened from Rust | Native | Native |
| Runtime detection | `lab/runtime.rs` using `programs` | Search path plus app bundle locations | Search path plus `%LOCALAPPDATA%` |
| Diagnostics and feedback | `feedback` | Redacted report; fixed GitHub links opened by the core | Same |

The UI has one helper, `src/lib/platform.ts`, for modifier keys, shortcut labels and file names.

**Design system.** Tokens are defined once in `src/index.css` (`@theme`): four surface levels from near-black to graphite, two border strengths, three text levels, mint (`accent`, also `ok`) for success, live state and primary actions, lavender (`lav`) for focus rings, selection and intelligence/model screens, `warn` and `danger` for status, four radii and two font stacks. Components use only these tokens; palette classes and hex colours in components were removed.

**Release pipeline.** `.github/workflows/`: `pr.yml`, `ci.yml`, `release.yml` (draft releases from tags), `pages.yml`, and the shared `_build.yml`. Details and the verification table are in [RELEASES.md](RELEASES.md).

**Not verified.** No Windows or Linux machine or virtual machine was available during development. Windows code was type-checked with a cross-compiler; neither system has run NEXUS until the workflows run.
