# Reliability audit

November 2026. Companion to [SECURITY_AUDIT.md](SECURITY_AUDIT.md). Bugs are listed with reproduction in [BUGS.md](BUGS.md).

## Findings

| ID | Sev | Area | Finding | Status |
| --- | --- | --- | --- | --- |
| PROC-1 | P2 | Process runner | Unbounded buffering of output without newlines; steps hung by leftover background processes | Fixed |
| AGENT-1 | P2 | Agents / projects | Removing a project under a running agent, and leaking its worktrees | Fixed |
| DB-1 | P2 | Database | Newer schemas opened blindly; no backup before migration; damaged database ended start-up silently | Fixed |
| TASK-1 | P2 | Tasks | Commands ran in the process working directory when the project folder was gone | Fixed |

### PROC-1

- **Files:** `src-tauri/src/jobs.rs`; affects every task step and agent test command.
- **Root cause 1:** output was read with `read_until(b'\n')`. The 200 KB cap applied per line *after* a line was complete, so one long line (binary output, a progress bar, minified code) grew in memory without limit first. Reproduced with 20 MB on one line.
- **Root cause 2:** the loop ended only when every holder of the output pipe closed it. `server &` (or any daemon that keeps stdout) kept a finished step "running" forever.
- **Fix:** 8 KB reads with UTF-8 carry-over (a character split between reads is never mangled); the cap is applied per chunk. When the command exits, leftovers get 3 s, then the process group is stopped and the output says so; if something outside the group still holds the pipe, NEXUS stops waiting after another 3 s. Cancel returns immediately as before. Processes that redirect their own output (`nohup … > log &`) do not hold the pipe and are left alone.
- **Tests:** `output_without_newlines_is_bounded`, `a_command_that_leaves_a_background_process_still_finishes`, `cancelling_stops_the_command_and_its_children`, `utf8_is_never_split_across_reads`.

### AGENT-1

- **Files:** `src-tauri/src/projects.rs` (`remove`), `agents/mod.rs`, `terminal.rs`.
- **Root cause:** `remove_project` deleted the project row; `ON DELETE CASCADE` removed its agent runs and steps while a run could still be executing, so the engine went on writing to rows that no longer existed. Each run's Git worktree stayed on disk and registered in the user's repository (`git worktree list`). Shells kept running until the next terminal call.
- **Fix:** removal is refused while an agent task is queued, running or paused ("cancel it in Agents first"); otherwise each run's worktree is removed through Git first; the project's shells are stopped immediately (existing `stop_orphans`).
- **Test:** `removing_a_project_cleans_up_agent_workspaces_and_refuses_while_work_runs`.

### DB-1

- **Files:** `src-tauri/src/db.rs`, `src-tauri/src/lib.rs`.
- **Root causes:** `user_version` greater than the known migrations was skipped silently (a downgraded app would run against an unknown schema); no copy was taken before migrating; a damaged file failed `setup` and the app exited with the reason on stderr only; no busy timeout.
- **Fix:** newer schemas are refused and left untouched; before migrating an existing database, `VACUUM INTO nexus.db.before-v<N>` writes a consistent copy (including WAL contents) and an existing copy is not overwritten; each migration step and its version commit in one transaction, so an interrupted upgrade resumes; 5 s busy timeout; a damaged or foreign file produces a readable message in a native dialog and is not modified.
- **Tests:** `an_upgrade_backs_up_the_database_first`, `a_newer_schema_is_refused_and_left_alone`, `a_damaged_file_is_reported_not_overwritten`, plus the existing `upgrading_an_existing_database_keeps_user_data`, `every_migration_reverses_cleanly`. Manually: the debug build launched on a 20 KB random-bytes `nexus.db` in a throwaway `NEXUS_DATA_DIR` showed the dialog; the file's SHA-1 was unchanged afterwards.

### TASK-1

See [SECURITY_AUDIT.md](SECURITY_AUDIT.md#task-1-p2--approved-commands-could-run-in-the-wrong-directory).

## State machines

### Agent run

```
queued ──▶ running ──▶ completed
   ▲          │ ├────▶ rejected    (plan not approved)
   │          │ ├────▶ discarded   (changes not applied)
   │          │ ├────▶ failed      (step failed after retries)
   │          │ └────▶ cancelled
   │          ▼
   │        paused ──(resume)──▶ running
   │
   └──(resume)── failed | interrupted | paused
interrupted  ◀── running | queued | paused   (app restarted)
```

- **Atomicity:** one SQLite connection behind a mutex. Every status change is a single `UPDATE`. The end-of-run close-out (open steps → cancelled/skipped, then the run status) happens in the same lock scope.
- **Duplicate execution:** `pump` starts only `queued` runs not already in the active map. `agent_resume` on an active run only un-pauses.
- **Steps:** `pending → running → completed | failed`, `pending → awaiting_approval → running | skipped | rejected`, any open step → `cancelled` on cancel.
  - A step whose dependency failed, was rejected or was cancelled is not started (`next_step`).
  - On resume, failed or cancelled non-command steps return to `pending`. Command steps are not re-run without a new approval.
- **Failure is never reported as success:**
  - A `Stop::Failed` sets `failed` with the message.
  - Command steps are `completed` only on exit code 0.
  - Provider errors are retried with backoff, then fail the run.
  - Malformed model replies are retried and then fail (`parse_reply` tests).
- **Cancellation** is honoured while waiting for a model, while awaiting approval, and during a command (process group killed). Tests: `agents::tests` (cancel, pause, resume, restart recovery, scripted and broken models).

### Task run

`pending → awaiting_approval → running → completed | failed | cancelled`, with `skipped` for rejected steps.

- Only one run per task (`run_task` refuses a second; tested).
- Approvals are checked against the step's stored status (new).
- A missing working directory fails the step without running it (new).
- After a restart, running steps are recovered (`steps_are_ordered_and_recovered`).

## Terminal and processes

- **PTY reading:** 8 KB reads, UTF-8 carry-over to the window, raw bytes to the scrollback file.
- **Scrollback:** capped at about 1–2 MB per session, and replayed by byte offset so the replay and the live stream never duplicate.
- **Limits:** 8 live shells (`MAX_LIVE`).
- **Exit codes:** recorded by a waiter thread; a session the user stopped is recorded without a misleading code.
- **Clean-up:** orphaned shells (project removed) are stopped. At start-up, sessions from the previous run are marked ended and scrollback files without a session are deleted.
- **Input:** only keyboard and paste reach a shell; no agent, task or model path writes to a terminal.
- **Model servers** started by NEXUS are stopped on quit and on SIGTERM/SIGINT/SIGHUP. Servers left by a forced exit are reaped at the next start (existing tests).
- **Not changed:** commands have no time limit; they can be cancelled (security audit R7).

## Indexing and search

- **Walkers:** honour `.gitignore`, skip generated folders (`node_modules`, `target`, `dist`, `.venv` …), never follow links, and stop at 60 000 files.
- **Size and content limits:**
  - Text is read up to 1.5 MB; NUL bytes in the first 8 KB mark a binary file.
  - Search scans files up to 64 MB line by line.
  - Regexes are linear-time (Rust `regex`) with a 2 MB compiled-size limit.
- **Index:** only source files of supported languages are indexed, so `.env`, keys and certificates are never indexed. It is replaced atomically in one transaction (`store_index`).
- **Edge-case test** (`stress_tests::awkward_projects_are_handled`): 60-level nesting; names with spaces, accents, CJK and emoji; invalid UTF-8 inside a source file; empty, binary and 2.9 MB files; an unreadable file; a broken link; a link loop; a self-link; a link out of the project; `.env`.
  - Analysis, indexing and search finish without error.
  - They stay inside the project.
  - Symbols on both sides of the invalid bytes are found.
- **Repository-state test** (`unusual_repositories_report_their_state`): a non-repository, a repository with no commits, a detached HEAD, and a `.git` file with nonsense. Each gives a status or a readable error.
- **Not provided:** a file watcher. The index is rebuilt on request (documented limit).

## Performance baseline

Release build, Apple M5 Pro, 20 000 generated source files in 400 folders (half TypeScript, half Rust, each about 1.2 KB, with cross-folder imports). Measured by `stress_tests::load_baseline` (`cargo test --release --lib stress_tests -- --ignored --nocapture`).

| Operation | Time |
| --- | ---: |
| Analyse project (languages, dependencies, findings) | 398 ms |
| Build code index (20 000 symbols, 10 000 imports) | 547 ms |
| Store code index (one transaction) | 31 ms |
| Symbol search | 1 ms |
| Content search, literal, all files | 375 ms |
| Content search, regex, all files | 83 ms |
| `git status` (20 changed files) | 48 ms |

- **Start-up:** with a fresh data folder, the release app had created and migrated its database 1.0 s after launch (`scripts/smoke-launch.mjs`, which polls every 0.5 s).
- **Threading:** all of these operations run on blocking worker threads, not the UI thread (`spawn_blocking` / async commands).
- **Conclusion:** nothing here materially affects the experience at this size, so no optimisation was made.
- **Not measured:**
  - repositories much larger than 60 000 files (the walker stops there by design);
  - UI rendering of very long lists. The Git file list renders every changed file without virtualisation; 132 changed files rendered instantly, but tens of thousands would be slow.

## Frontend

- **Subscriptions and timers:** every Tauri event subscription is unsubscribed in its effect's cleanup (13 subscriptions checked), and every interval and window listener is cleared.
- **Async work:** asynchronous loads that finish after unmount are ignored (`disposed` flags in Terminal).
- **Rendering:** no HTML sinks; the earlier redesign's hook-order bug in Git was already fixed.
- **Manual check:** ten screens (all except Settings) opened in the debug build against the real data, with no error banners. Git showed tracked and untracked diffs with the new flags and path checks.


---

# Security 2.0 — process lifecycle, time limits and resource limits

## Findings

| ID | Sev | Finding | Fix | Tests |
| --- | --- | --- | --- | --- |
| LOCK-1 | P2 | **Deadlock** (pre-existing): when a task step started waiting for approval, the task loop sent the desktop notification while holding the database lock. The notification asks the main thread whether the window is focused and waits; a UI refresh on the main thread was waiting for the database. The app froze. Found by sampling the hung debug build during the UI check. | Notification sent after the lock is released; a warning on `notify::attention` | Not reproducible on the mock runtime (focus checks do not block there); verified in the real app: the same flow completes |
| TIME-1 | P2 | No time limits | Per-level policy time limit; default 30 min for task steps, 15 min for agent commands, per-task override (`tasks.timeout_secs`, migration 6, reversible), 5 s – 24 h | `a_command_past_its_time_limit_is_stopped_with_everything_it_started`, `the_time_limit_applies_on_every_system`, `a_task_time_limit_stops_a_hanging_step`, `a_hanging_agent_command_times_out_and_fails_the_step` |
| PROC-2 | P2 | Cancel killed only the process group: a child that moved to its own session (`setsid`, daemonising helpers) survived; processes ignoring SIGTERM were not an explicit case | One stop routine for timeout and cancel: SIGTERM to the group and every descendant (found by walking parent ids), 2 s grace, then SIGKILL; processes seen earlier that became orphans are killed too, without touching reused pids | `a_child_that_left_the_process_group_is_stopped_too`, `a_command_that_ignores_termination_is_killed`, `cancelling_stops_the_command_and_its_children` |
| PROC-3 | P2 | If NEXUS crashed or was killed, running task and agent commands continued (they run in their own process group) | Running commands are recorded (`running-jobs.json`); the next start kills members of those groups that are not older than the job; quitting (and SIGTERM/SIGINT/SIGHUP) stops all running commands | `commands_left_by_a_crash_are_stopped_at_the_next_start` (an unrelated process survives), `stopping_everything_ends_running_commands`, `leftovers_from_a_crash_are_identified_by_group_and_age` |
| LIM-1 | P3 | Limits were constants or missing | Configurable in Settings → Execution; refusals name the limit | `settings_are_validated_and_bad_stored_values_fall_back` |
| ENV-2 | P2 | Clearing the model server's environment after setting `OLLAMA_HOST` (introduced during this work, caught by the existing runtime test before it shipped) | The policy environment is set first | `a_managed_server_is_started_without_a_shell_logged_and_stopped` |

## Database state after a stop

| Situation | Step | Run / task | Exit code |
| --- | --- | --- | --- |
| Time limit passed (task) | `failed`, output ends with the reason | `failed`; later steps stay `pending` | none |
| Time limit passed (agent command) | `failed`, error `timed out after …` | not `completed` | none |
| Cancelled | `cancelled` | `cancelled` | none |
| App quit or crashed | running or waiting steps `cancelled` | next start: tasks `cancelled` (Reset to run again; seen in the UI check), agent runs `interrupted` (Resume asks for approval again) | — |

## Process lifecycle across the app

| Process | Started by | Group / session | Stopped on cancel / timeout | Stopped on quit | After a crash |
| --- | --- | --- | --- | --- | --- |
| Task step, agent command | `jobs::run_command` | own group | group + descendants, TERM → KILL | yes (`jobs::stop_all`) | reaped at next start |
| Sandboxed command (Linux) | bubblewrap | own group, own PID namespace | as above; the namespace dies with its init | yes; `--die-with-parent` | dies with NEXUS |
| Terminal shell | PTY | own session | `kill_session`: HUP, then KILL to the session | PTY closes → HUP | PTY closes → HUP; sessions marked ended at next start |
| Model server | `lab::runtime` | own group | TERM → KILL | yes (`lab::shutdown`) | reaped at next start (existing) |
| Git | `git()` | inherits | runs to completion (short) | — | — |
| Login-shell PATH probe | `platform::env` | inherits | killed after 4 s | — | — |

Zombies: every child is waited on (`child.wait()` in the runner, a waiter thread for PTYs, the runtime monitor for servers).

## Limits

| Limit | Default | Configurable | Behaviour when exceeded |
| --- | --- | --- | --- |
| Task step time | 30 min | yes, and per task | tree stopped, step failed with reason |
| Agent command time | 15 min | yes | tree stopped, step failed |
| Output kept per command | 200 KB | 16 KB – 10 MB | cut, note in output; time-limit and stop notes always kept |
| Largest file written (Restricted, Isolated) | 4 GB | yes | write fails (`EFBIG`) inside the command |
| Memory (Isolated, Linux) | 8 GB | yes | allocation fails inside the command |
| Agent tasks at once | 2 | 1–8 | others wait in the queue |
| Agent tasks queued | 20 | 1–200 | creation refused with the reason |
| Task runs at once | 4 | 1–32 | run refused with the reason |
| Live terminals | 8 | no | opening refused |
| Provider / API reply | 32 MB | no | request refused |
| Error bodies | 64 KB | no | cut |
| Agent file writes | 40 files, 400 KB each | no | step fails |
| Indexing | 60 000 files, 1.5 MB per text file | no | the rest is skipped and counted |

## Failure tests added (all on temporary fixtures)

Hanging command; command spawning children; command ignoring SIGTERM; child escaping with `setsid`; continuous output (`yes`); 20 MB single line; missing folder; missing program; non-executable script; leftover background process; cancellation; four agent runs cancelled concurrently; app shutdown; crash leftovers; provider that does not answer (`a_provider_that_does_not_answer_times_out`); environment leakage at each level; sandbox write, read, keychain and network denial.
