# Remediation record

What was changed, why, and how it was verified. Nothing has been committed.

## Code changes

| File | Change | Findings |
| --- | --- | --- |
| `src-tauri/src/git.rs` | Repository-configured helpers off for every NEXUS Git call; no prompts; untracked preview contained; diff and status logic testable outside the Tauri command | GIT-1, GIT-2 |
| `src-tauri/src/agents/workspace.rs` | Hooks disabled when creating an agent worktree | AGENT-2 |
| `src-tauri/src/projects.rs` | `remove` refuses under running agent work, removes worktrees, stops shells; `mask_secrets` | AGENT-1, SEC-1 |
| `src-tauri/src/agents/roles.rs`, `intel/mod.rs` | Read-only prompts masked | SEC-1 |
| `src-tauri/src/tasks.rs` | Explicit working directory; approvals checked against stored status | TASK-1, TASK-2 |
| `src-tauri/src/jobs.rs` | Chunked, UTF-8-safe reading; leftover-process handling; cancel unchanged | PROC-1 |
| `src-tauri/src/ai.rs` | `http_client` with a same-origin redirect policy; `read_body`, `read_body_prefix`; readable redirect error | NET-1, NET-2, NET-3 |
| `src-tauri/src/lab/{registry,infer,download,runtime}.rs` | Bounded body reads | NET-2 |
| `src-tauri/src/db.rs` | Newer-schema refusal, pre-migration backup, busy timeout, readable errors | DB-1 |
| `src-tauri/src/lib.rs` | Start-up dialog when the database cannot be opened; shared client; test module | DB-1, NET-1 |
| `src-tauri/src/terminal.rs`, `agents/mod.rs`, `search.rs` | Visibility changes for the above and for tests | — |
| `src-tauri/src/stress_tests.rs` (new) | Edge-case fixtures and the load baseline | Phases 8–10 |
| `src-tauri/Cargo.toml`, `Cargo.lock` | `rusqlite` 0.41, `url` 2.5.9, `cookie` 0.18.3 | DEP-1 |
| Several Rust files | Clippy fixes / reasoned `allow`s | LINT-1 |
| `.github/workflows/pr.yml`, `_build.yml` | Clippy with `-D warnings`; dependency-security job (`cargo audit`, `npm audit`, lockfile checks) | Phase 12 |

## Verification

| Check | Before the audit | After |
| --- | --- | --- |
| `cargo test` | 157 passed | **174 passed**, 1 opt-in load test |
| `npm test` | 21 passed | 21 passed |
| `npm run typecheck` / `npm run build` | pass | pass |
| `cargo fmt --check` | pass | pass |
| `cargo clippy --all-targets -- -D warnings` | 15 warnings | **0** |
| `cargo audit` | 3 vulnerabilities, 6 notices | **0 vulnerabilities**, 6 notices (no fix available; reviewed) |
| `npm audit` | 0 | 0 |
| Release build + `scripts/smoke-launch.mjs` | — | pass (database ready 1.0 s after launch) |
| Manual, debug build on real data | — | ten screens opened (all except Settings), no errors; Git diffs (tracked and untracked) correct |
| Manual, damaged database | — | dialog shown, file unchanged |

New tests (17): `db` ×3, `git` ×2, `jobs` ×4, `agents::workspace` ×1, `projects` ×2, `tasks` ×1, `ai` ×2, `stress_tests` ×2 (+1 ignored load test).

## Not verified

- **Windows and Linux:** nothing from this audit has run there. The changes were built and tested on macOS only; CI runs on push or pull request, and nothing has been pushed.
  - Tests that need a Unix shell or Unix permissions are `#[cfg(unix)]` (GIT-1, AGENT-2, PROC-1, the awkward-project fixture).
  - The Windows path for AGENT-2 (`core.hooksPath=NUL`) is untested.
  - The leftover-process handling uses `taskkill /T` on Windows, which cannot reach children once their parent has exited; leftovers there are cut off by the second grace period instead of being killed.
- **The new CI jobs** have not run yet; they were validated as YAML only. `taiki-e/install-action` is a new third-party action in CI.
- **Performance at very large scale:** UI performance with tens of thousands of changed files was not measured.

## Recommended next steps

1. Push a branch and let the pull-request workflow run on Windows and Linux; fix anything platform-specific it finds.
2. Scrub credential variables from task steps that came from AI planning (R3).
3. Keep files with detected secrets out of the Developer context (R2).
4. Offer an opt-in sandbox for agent test commands (R1).
5. Replace `chrono` (R6).
6. Optional per-step timeout for task and agent commands (R7).


---

# Security 2.0 — remediation record

Nothing has been committed.

## Code changes

| File | Change |
| --- | --- |
| `src-tauri/src/exec/mod.rs` (new) | Levels, availability, exact descriptions, `ExecSettings` (validated, stored under `exec`), `Policy`, commands `exec_capabilities`, `exec_settings_get`, `exec_settings_set` |
| `src-tauri/src/exec/env.rs` (new) | Environment policy per level; credential detection by name segment and by value; user allow-list |
| `src-tauri/src/exec/sandbox.rs` (new) | Backend probe; confinement per level; Seatbelt profile generator (macOS); bubblewrap arguments (Linux) |
| `src-tauri/src/secrets.rs` (new) | Structured credential detection, masking, `Vault` for protected editing |
| `src-tauri/src/jobs.rs` | Runner takes a `RunSpec` with a policy; sandbox wrapping; environment from scratch; resource limits; time limit; configurable output limit; `[NEXUS]` record line; running-job registry, crash reaping, `stop_all` |
| `src-tauri/src/platform/process.rs` | `descendants`, descendant-aware `kill_tree`, graceful `stop_tree` (TERM → grace → KILL, orphan-safe) |
| `src-tauri/src/tasks.rs` | Policy per step, per-task time limit (`set_task_timeout`), running-task limit, deadlock fix (notification outside the database lock), risk rule fix |
| `src-tauri/src/agents/engine.rs`, `agents/mod.rs` | Agent policy for commands, timeout handling, configurable concurrency and queue limit, protected editing in the Developer step |
| `src-tauri/src/lab/runtime.rs` | Model servers get the Trusted environment policy (built before their own variables) |
| `src-tauri/src/projects.rs` | `mask_secrets` now uses the shared detector |
| `src-tauri/src/db.rs` | Migration 6: `tasks.timeout_secs` (reversible) |
| `src-tauri/src/lib.rs` | New modules and commands; job registry at start-up; stop running commands on quit and on signals |
| `src-tauri/src/platform/notify.rs` | Warning against calling it under a lock |
| `src/components/ExecNote.tsx`, `src/views/settings/Execution.tsx` (new) | Approval note (level, backend, time limit); Settings → Execution |
| `src/views/Settings.tsx`, `Tasks.tsx`, `Agents.tsx`, `src/lib/api.ts` | Execution section; per-task time limit; notes on approval cards; API |
| `.github/workflows/pr.yml` | Linux installs bubblewrap and requires the sandbox tests to run; new macOS job for the execution, secret, agent and task tests |

## Verification

| Check | Result |
| --- | --- |
| `cargo test` (macOS) | **205 passed**, 1 opt-in load test ignored (31 new in this phase) |
| `cargo clippy --all-targets -D warnings` (macOS) | clean |
| `cargo clippy --all-targets -D warnings` for `x86_64-pc-windows-gnu` (cross-compiled locally with MinGW) | clean; this compiles every Windows code path and test but does not run them |
| `cargo fmt --check`, `npm run typecheck`, `npm test` (21), `npm run build` | pass |
| `cargo audit` / `npm audit --audit-level=high` | 0 vulnerabilities (6 notices, unchanged and reviewed) |
| Release build + `scripts/smoke-launch.mjs` | pass |
| Real database | the release app migrated it to schema 6 after writing `nexus.db.before-v6` (schema 5, same projects) |
| UI, debug build on a throwaway data folder and project | Settings → Execution saved Restricted for tasks; a task with a 5 min limit showed "Runs Restricted (macOS sandbox) · time limit 5 min" on its approval card; approved from the UI it wrote inside the project, could not list `~/.ssh`, could not write to the home folder (no file created), and its record starts with the protection it ran under. This check also found the deadlock (LOCK-1) and the writable-home case (EXEC-2), both fixed and re-checked |

## Not verified

- **CI has not run any of this.** The workflows only run on a pushed commit or a pull request, and nothing has been pushed. In particular:
  - **Linux:** the bubblewrap backend has never run. Its tests are written and CI is set to fail if they skip, but its behaviour (mount order, `--unshare-net` loopback, hiding `$XDG_RUNTIME_DIR`, `RLIMIT_AS`) is unverified.
  - **Windows:** compiled and linted locally, never run. The cross-platform time-limit test (`ping` under `cmd`) has not run there.
- **Agent approval note:** the same component as on task cards, but it was not seen with a live model, because no model was connected for the UI check.
