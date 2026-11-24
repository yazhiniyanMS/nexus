# Audit log

Running record of the security and reliability audit (started 2026-11-24). Newest entries last. A later session can resume from the last entry.

## Phase 0 — baseline

- Branch `main` at `c2b9cdb`. Working tree carries the uncommitted UI redesign (42 paths: 36 modified, 2 deleted, 4 new). A copy of it was saved before any audit change (tracked diff + untracked files) so nothing can be lost; the audit only adds to it.
- Rust: `cargo fmt --check` clean. `cargo test`: **157 passed**, 0 failed. `cargo clippy --all-targets`: 15 warnings, all style lints (`sort_by_key`, complex types, too many arguments, needless borrows), no errors.
- Frontend: `npm run typecheck` clean; `npm test`: **21 passed**; `npm run build` succeeds (largest chunk: Monaco file viewer, 3.3 MB, lazy-loaded).
- `npm audit` (all and production only): 0 known vulnerabilities.
- `cargo audit` / `cargo deny` were not installed; `cargo-audit` installed for the audit.
- No pre-existing failures.

## Entries
- **DB-1 (P2)** `db.rs`: a database from a newer NEXUS was opened without a check; no backup before migrations; a damaged file ended start-up with the reason on stderr only. Fixed: newer schema refused untouched, `VACUUM INTO nexus.db.before-v<N>` before migrating an existing database, 5 s busy timeout, readable error + native dialog at start-up (checked by launching the debug build on a random-bytes database in a throwaway `NEXUS_DATA_DIR`; file hash unchanged). Tests: `an_upgrade_backs_up_the_database_first`, `a_newer_schema_is_refused_and_left_alone`, `a_damaged_file_is_reported_not_overwritten`.
- **DEP-1** `cargo audit`: `url` 2.5.8 (RUSTSEC-2026-0592), `cookie` 0.18.2 (RUSTSEC-2026-0594), `libsqlite3-sys` 0.30.1 (RUSTSEC-2026-0459). Updated `url` 2.5.9 and `cookie` 0.18.3 (lockfile only); `rusqlite` 0.32 → 0.41 (bundles SQLite 3.54.1). Only API change needed: schema version read as `i64` instead of `usize`.
- **GIT-1 (P1)** `git.rs`: NEXUS runs Git automatically when a project is shown; a repository's own `.git/config` could name `core.fsmonitor`, `diff.external` or a `textconv` driver and so run a program without the user asking. Fixed: `-c core.fsmonitor=false` on every call, `--no-ext-diff --no-textconv` on diff and show, `GIT_TERMINAL_PROMPT=0`, stdin closed. Test `repository_config_cannot_make_reading_run_programs` (with a control proving plain Git does run the helper).
- **GIT-2 (P2)** `git.rs` `git_diff` for untracked files joined the UI path onto the root without containment (absolute paths and `..` read any file). Fixed with `resolve_inside`. Test `untracked_previews_stay_inside_the_project`.
- **PROC-1 (P2)** `jobs.rs` (all task and agent commands): output was read line by line, so a command printing without newlines grew one string without limit before the 200 KB cap applied; and a command leaving a background process that held its output open never finished. Fixed: 8 KB reads that never split UTF-8 characters; once the command exits, leftovers still holding its output get 3 s and are then stopped (process group), with a note in the output; cancel returns at once even if something outside the group holds the pipe. Tests: `output_without_newlines_is_bounded` (20 MB single line), `a_command_that_leaves_a_background_process_still_finishes`, `cancelling_stops_the_command_and_its_children`, `utf8_is_never_split_across_reads`.
- **AGENT-2 (P2)** `agents/workspace.rs`: `git worktree add` ran the project's `post-checkout` hook each time an agent task started. Fixed with `core.hooksPath` pointed at a path that cannot exist. Test `creating_a_workspace_runs_no_repository_hooks` (with control).
- **AGENT-1 (P2)** `projects.rs` `remove_project`: cascaded deletes under a running agent (engine kept writing to deleted rows) and left agent worktrees on disk and registered in the user's repository. Fixed: refuse while an agent task is queued/running/paused; remove each run's worktree first; stop the project's shells right away (existing `stop_orphans`) instead of on the next terminal call. Test `removing_a_project_cleans_up_agent_workspaces_and_refuses_while_work_runs`.
- Checked and sound: agent path containment (`safe_join`: no `..`, absolute, `.git`, symlinks; size and count caps); approvals bound to a step id and checked against the database (`agent_decide` refuses steps not awaiting approval); the applied patch is the one computed before approval; capability checks in the backend (`roles::require`); restart recovery (running → interrupted, interrupted commands are asked again).
- **TASK-1 (P2)** `tasks.rs`: when a task's project folder was gone (moved, deleted) or the task had no project, approved commands ran in NEXUS's own process directory (`/` for the macOS app). Fixed: a missing project folder fails the step without running it; a task without a project runs in the home folder. `approve_step` now refuses steps that are not awaiting approval (as `agent_decide` already did). Test `a_missing_project_folder_stops_the_command_and_early_approvals_are_refused`.
- Tests after this batch: 169 Rust tests pass.
- **NET-1 (P2)** `ai.rs` shared HTTP client followed redirects to any host. reqwest strips `Authorization` across hosts but not `x-api-key` (Anthropic), so a redirect from a configured endpoint would carry the key to another server, possibly over plain HTTP. Fixed: `ai::http_client()` follows redirects only within the same scheme, host and port (max 5) with a readable refusal. Test `credentials_do_not_follow_redirects_to_another_server` (the other server receives nothing; same-server redirects still work).
- **NET-2 (P3)** Provider, Hugging Face API and probe replies were read whole (`.text()`/`.json()`); a base URL pointing at the wrong server could make NEXUS buffer an unbounded body. Fixed: `read_body` (32 MB cap, readable error) and `read_body_prefix` (64 KB for error bodies) used at every site. Test `oversized_replies_are_refused_without_reading_them_whole`.
- **SEC-1 (P2)** Source excerpts sent to models (Ask, agent Research/Architect/Review) carried hard-coded credentials verbatim. Fixed: `projects::mask_secrets` masks secret assignments (values only), AWS/GitHub/Slack/OpenAI/Anthropic/Google key formats and private-key blocks. The Developer prompt is deliberately not masked (its output is written back to files). Test `credentials_are_masked_before_text_goes_to_a_model`.
- Secrets checked: keys live only in the OS keychain (SQLite has a `has_key` flag); `list_providers` never returns keys; error text is redacted; the only key-shaped string in the repository and its history is a labelled fake in the redaction test. `.env`, key and certificate files are never indexed (the index takes source files of supported languages only).
- Tests after this batch: 172 Rust tests pass.
- **Phases 8–10** `stress_tests.rs` (new): awkward-project fixture (nesting, Unicode, links, loops, invalid UTF-8, huge/binary/unreadable files) and unusual repositories (none, no commits, detached HEAD, damaged `.git`) — no defects. Load baseline, release build, 20 000 files: analyse 398 ms, index 547 ms, store 31 ms, symbol search 1 ms, literal search 375 ms, regex search 83 ms, `git status` 48 ms. Release start-up: database ready 1.0 s after launch.
- **Phase 12** CI: clippy with `-D warnings` on PRs and builds (15 existing warnings fixed or annotated with a reason); new "Dependency security" job (`cargo audit`, `npm audit --audit-level=high`, lockfile checks). Not yet run in CI (nothing pushed).
- **Final state:** 174 Rust tests + 1 opt-in load test, 21 frontend tests, type-check, build, fmt, clippy, `cargo audit` (0 vulnerabilities) and `npm audit` all pass on macOS. Windows and Linux not verified. Documents: THREAT_MODEL, SECURITY_AUDIT, RELIABILITY_AUDIT, BUGS, REMEDIATION. Nothing committed.

## Where to resume

Remaining recommendations, in order: run the PR workflow on Windows/Linux; scrub credentials for AI-planned task steps (R3); keep secret-bearing files out of the Developer context (R2); opt-in sandbox for agent test commands (R1); replace `chrono` (R6); optional command timeouts (R7).


## Security 2.0 — execution isolation (2026-11-24)

- Read the remaining risks: R1 unsandboxed commands, R2 Developer prompt unmasked, R3 task environment, R7 no time limits.
- **Probed this Mac:**
  - `sandbox-exec` is present and enforces file-write, file-read, network and mach-lookup rules on macOS 27.2.
  - Canonical paths are required (`/private/var/...`).
  - No Docker, Podman or Lima.
  - A MinGW cross-compiler and the `x86_64-pc-windows-gnu` target are installed, so Windows code can be compiled and linted locally but not run (no Wine).
  - `ps -E` does not show other processes' environments on macOS, so crash reaping uses process group + age instead of an environment marker.
- **Designed and built** the `exec` module (levels, policy, backends), `secrets` (detector + vault), the runner rewrite, process-tree stop, job registry, settings and UI.
- **Found while testing:**
  - My own `stop_all` test killed other tests' commands. It now stops only its own command.
  - The broad loopback rule opened outbound networking in Isolated. It was replaced before shipping.
  - The model-server environment order bug (ENV-2) was caught by the existing runtime test.
  - The `.env` rule caught code assignments. It is now limited to `KEY=value` lines without spaces.
  - Windows-only lint failures were found by the cross-compile.
- **UI check** on a throwaway data folder found LOCK-1 (deadlock; app froze, diagnosed with `sample`) and EXEC-2 (home writable for project-less Restricted tasks). Both fixed and re-checked. The `>/dev/null` risk false positive was also fixed.
- **Final local state:** 205 Rust tests, 21 frontend tests, type-check, build, fmt, clippy (macOS and Windows cross), `cargo audit`, `npm audit` all pass. Release app rebuilt, smoke-tested and relaunched; it backed up and migrated the real database (schema 5 → 6).
- **Throwaway data removed** (`ui-data`, `ui-project`, the sandbox probe folder). The release app was quit during the UI check (both processes are named `nexus`) and relaunched at the end.

### Where to resume

1. Push a branch and run the pull-request workflow (Linux with bubblewrap, Windows, macOS execution job); fix what it finds. **Needs the user's go-ahead: they asked for no commits.**
2. Windows isolation backend (restricted token or AppContainer, job object for memory and process limits) (R11).
3. A level with a private HOME, so reads of the rest of the home folder are blocked too (R9).

## CI verification (2026-11-24, branch `prepare-0.2`)

The `CI` workflow (full build: fmt, clippy `-D warnings`, type-check, tests, installers, install and launch checks) was run on the branch for all systems.

- Run 45403394508: macOS Apple Silicon and Intel passed. Linux failed clippy (`unused_mut` in `ollama_binary`, Linux-only). Windows failed two tests: a database test deleted its folder while the file was still open (Windows forbids it), and an empty `NEXUS_REQUIRE_SANDBOX` was treated as "required".
- Run 45406218106 (fixes in `74da8de`): macOS ×2 and Windows passed. Linux: bubblewrap ran and all other sandbox tests passed, but a write into the hidden NEXUS data folder succeeded — into bubblewrap's empty tmpfs layer, not the real folder. Fixed in `c8cfee3` by remounting hidden layers read-only, so such writes fail, as on macOS.
- Run 45409321258 (Linux only, `c8cfee3`): passed, including the bubblewrap isolation tests (required via `NEXUS_REQUIRE_SANDBOX`).

The Windows and macOS jobs ran on `74da8de`; `c8cfee3` changes only the bubblewrap arguments and a pure unit test of them, which passes locally on macOS.
