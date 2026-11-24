# Security audit

November 2026. Scope: the whole repository at `c2b9cdb` plus the uncommitted UI redesign. Method: reading every place that runs a process, touches the filesystem, makes a network request or handles a credential; reproducing each suspected issue with a test against real Git, real processes, real HTTP servers and real SQLite files in temporary folders; fixing it; keeping the test. The threat model is in [THREAT_MODEL.md](THREAT_MODEL.md); the running record in [AUDIT_LOG.md](AUDIT_LOG.md).

Severity scale: P0 critical (arbitrary privileged execution, data loss), P1 major, P2 reliability/state corruption or significant weakness, P3 lesser, P4 polish.

## Findings

| ID | Sev | Area | Finding | Status |
| --- | --- | --- | --- | --- |
| GIT-1 | P1 | Command execution | Repository-configured programs ran when a project was merely shown | Fixed |
| GIT-2 | P2 | Filesystem | Untracked-file preview read any path | Fixed |
| NET-1 | P2 | Secrets / network | API key (`x-api-key`) followed redirects to other hosts | Fixed |
| SEC-1 | P2 | Secrets | Hard-coded credentials in source went to models verbatim | Fixed (read-only prompts); residual R2 |
| AGENT-2 | P2 | Agents / Git | Starting an agent task ran the project's `post-checkout` hook | Fixed |
| TASK-1 | P2 | Command execution | Approved task commands could run in NEXUS's own working directory | Fixed |
| PROC-1 | P2 | Processes | Unbounded output buffering; leftover background processes hung steps | Fixed |
| NET-2 | P3 | Network | Unbounded HTTP reply bodies | Fixed |
| DEP-1 | P3 | Supply chain | `url` stack overflow (RUSTSEC-2026-0592), SQLite page-cache disclosure (RUSTSEC-2026-0459), `cookie` nonce reuse (RUSTSEC-2026-0594) | Fixed (upgrades) |

No P0 was found.

### GIT-1 (P1) — repository config could make NEXUS run programs

- **Files:** `src-tauri/src/git.rs`.
- **Root cause:** NEXUS runs `git status`, `git diff` and `git show` automatically (Overview, Git screen) inside imported folders. Git honours that repository's `.git/config`, where `core.fsmonitor` names a program Git runs on `status`, and `diff.external` / `diff.<driver>.textconv` name programs run on `diff`/`show`. `.git/config` is not transferred by `git clone`, but it is when a repository arrives as an archive, a synced folder or a shared drive. Viewing such a project ran a program with the user's rights and no prompt.
- **Fix:** every NEXUS Git call passes `-c core.fsmonitor=false`; diff and show pass `--no-ext-diff --no-textconv`; `GIT_TERMINAL_PROMPT=0` and closed stdin so nothing waits for invisible prompts. User-initiated `git commit` still runs the repository's hooks, as it does in a shell (residual R4).
- **Test:** `git::tests::repository_config_cannot_make_reading_run_programs` configures all three helpers to write a marker file, runs status/diff/show through NEXUS, asserts no marker, then runs plain `git diff` as a control and asserts the marker appears.

### GIT-2 (P2) — path traversal in the untracked-file preview

- **Files:** `src-tauri/src/git.rs` (`git_diff`).
- **Root cause:** `root.join(path)` with a path from the UI; an absolute path replaces the root, `..` climbs out. Every other file read used `resolve_inside`.
- **Fix:** `resolve_inside` (canonicalise, then require the result to stay under the canonical root).
- **Test:** `git::tests::untracked_previews_stay_inside_the_project` (`../secret.txt` and an absolute path are refused; a normal file works).

### NET-1 (P2) — API keys could follow redirects

- **Files:** `src-tauri/src/ai.rs` (`http_client`), `src-tauri/src/lib.rs`.
- **Root cause:** the shared reqwest client used the default redirect policy (up to 10, any host). reqwest removes `Authorization` when a redirect changes host, but not other headers; Anthropic's key travels in `x-api-key`. A redirect from a configured endpoint (or a proxy) would have sent the key to a different server, including over plain HTTP.
- **Fix:** redirects are followed only when scheme, host and port stay the same (max 5); otherwise the request fails with an explanation ("use the final address as the base URL"). Model downloads keep their own stricter client (HTTPS only).
- **Test:** `ai::tests::credentials_do_not_follow_redirects_to_another_server` (the second server records no request; same-server redirects still work).

### SEC-1 (P2) — credentials in source excerpts sent to models

- **Files:** `src-tauri/src/projects.rs` (`mask_secrets`), `src-tauri/src/intel/mod.rs` (Ask), `src-tauri/src/agents/roles.rs` (Research, Architect, Review prompts).
- **Root cause:** excerpts were sent as-is. The analysis flagged hard-coded keys as findings but did not keep them out of prompts. For remote models this needs the user's explicit per-request permission, but the user permits "project content", not "my AWS key".
- **Fix:** values of secret-named assignments (also typed ones like `password: &str = "…"`), AWS/GitHub/Slack/OpenAI/Anthropic/Google key formats and private-key blocks are replaced with `[redacted]` in read-only prompts. Answers still cite the real excerpts in the UI.
- **Not masked:** the Developer prompt, because the model rewrites those files and masking would write `[redacted]` into the user's code (residual R2).
- **Test:** `projects::tests::credentials_are_masked_before_text_goes_to_a_model` (built from fragments so the repository contains no scanner-matching literal).

### AGENT-2 (P2) — repository hooks ran when an agent task started

- **Files:** `src-tauri/src/agents/workspace.rs`.
- **Root cause:** `git worktree add` runs `post-checkout`. The user asked for an agent task, not for their hooks to run against a workspace NEXUS manages.
- **Fix:** `-c core.hooksPath=/dev/null` (`NUL` on Windows) for that command only.
- **Test:** `agents::workspace::tests::creating_a_workspace_runs_no_repository_hooks` (with a control checkout that does run the hook).

### TASK-1 (P2) — approved commands could run in the wrong directory

- **Files:** `src-tauri/src/tasks.rs`.
- **Root cause:** `project_dir` returned `None` when the task had no project, or when the project's folder had been moved or deleted; the command then ran in the process working directory (`/` for the macOS app). A step such as `rm -rf build` approved for one tree would have run against another.
- **Fix:** a missing project folder fails the step before running it, with the reason shown; a task without a project runs in the home folder, explicitly. `approve_step` also now refuses steps that are not awaiting approval (defence in depth; the agent engine already did this).
- **Test:** `tasks::tests::a_missing_project_folder_stops_the_command_and_early_approvals_are_refused`.

### PROC-1 (P2) — process output and lifetime

Covered in [RELIABILITY_AUDIT.md](RELIABILITY_AUDIT.md#proc-1); listed here because an approved command (or a hostile `npm test`) could exhaust memory with output that has no newlines, and could keep a step "running" forever by leaving a background process holding its output.

### NET-2 (P3) — unbounded reply bodies

- **Files:** `src-tauri/src/ai.rs`, `lab/registry.rs`, `lab/infer.rs`, `lab/download.rs`, `lab/runtime.rs`.
- **Fix:** `read_body` (32 MB cap, error) for JSON replies; `read_body_prefix` (64 KB) for error bodies and probes.
- **Test:** `ai::tests::oversized_replies_are_refused_without_reading_them_whole`.

## Areas checked without a defect

**A. Command execution.** Every spawn site was read: `git.rs` (argument arrays, `--` before paths, commit hashes validated as hex), `jobs.rs` (the only path for task and agent commands; shell by design, because commands are shell lines the user approves), `terminal.rs` (the user's own interactive shell through a PTY; only keyboard input reaches it), `lab/runtime.rs` (fixed binaries, argument arrays, allow-listed model names, `127.0.0.1` bind), `platform/*` (fixed probes: `ps`, `pgrep`, `taskkill`, `git --version`, the login shell for `PATH`). No user or model text is ever interpolated into a command line other than the approved command itself. Agent-proposed commands are inserted as separate steps, each awaiting approval of its exact text; the risk classifier flags deletion, history rewrites, privilege escalation and pipe-to-shell.

**B. Filesystem.** UI paths: `resolve_inside` everywhere (`read_file`, `list_dir`, terminal working directory, and now the untracked diff). Model paths: `safe_join` rejects empty, NUL, `..`, absolute and prefix components, any `.git` component (case-insensitive) and any symlink along the path, then re-checks the canonical parent. Model files: `safe_name` allow-list, `.gguf` only, Windows device names refused, deletion only of regular files NEXUS downloaded. Walkers never follow links (tested with a link out of the project, a self-link and a link loop). Downloads write `*.part` then rename. Remaining weakness: canonicalise-then-open is not atomic (R5).

**C. Agents.** Permissions are enforced in the backend: every step handler calls `roles::require` before acting, and the role matrix is tested (`permission_matrix_is_exact`); no role has both write and run. `agent_decide` checks the step's stored status, so an early or repeated approval cannot apply to a later step. The patch applied is computed before the approval. Cancellation reaches model calls, approvals and commands (`select!` on the cancel channel; process groups killed). After a restart, running work becomes `interrupted`, and commands that were running are asked for again rather than re-run. Prompt injection from repository text can change what a model proposes, but not what runs without approval (R1, R10).

**D. Tauri.** Capabilities: `core:default`, `dialog:default` only. No `shell`, `fs` or `opener` permissions for the webview; `opener` is used from Rust with URLs built from a fixed list (the webview passes a kind, not a URL). Production CSP: `default-src 'self'; script-src 'self'`, connect only to IPC. DevTools: the `devtools` feature is not enabled, so release builds have none. No remote origins are granted IPC. The UI test bridge is `#[cfg(debug_assertions)]` and binds `127.0.0.1` only when `NEXUS_E2E_PORT` is set.

**E. Secrets.** Keys are written only to the OS keychain (`platform::secrets`); SQLite stores `has_key`. No command returns a key to the webview. Provider errors are passed through `redact` (the key itself, `sk-…` shapes, bearer tokens, URL credentials and query strings). Base URLs may not contain credentials (tested). Agent commands and model servers get no credential-like environment variables (`is_secret_env`, tested). Terminals inherit the user's environment by design (it is their shell). The diagnostic report lists versions and counts only. History scan: the only key-shaped string ever committed is a fake key labelled `FAKE` in the redaction test.

**F. Network.** HTTPS through rustls (no certificate bypass anywhere: no `danger_accept_invalid_certs`). Connect timeout 10 s on every request; request timeouts on all non-streaming calls; streams can be cancelled. Downloads: HTTPS only (loopback allowed), redirects must stay HTTPS, Hugging Face names validated, SHA-256 checked when published. Remote vs local endpoints are told apart (`is_local_url`, tested) and remote use of project content needs explicit permission (tested). No update mechanism and no telemetry exist.

## Dependencies and supply chain

- `cargo audit` before: 3 vulnerabilities (`url` 2.5.8, `libsqlite3-sys` 0.30.1, `cookie` 0.18.2) and 6 notices. After: **0 vulnerabilities**.
  - `url` → 2.5.9 and `cookie` → 0.18.3: semver-compatible lockfile updates. `cookie` was not exploitable here (no private cookie jars, no forking); updated because it was free.
  - `rusqlite` 0.32 → 0.41 (bundles SQLite 3.54.1). The SQLite issue needs a crafted database/WAL pair in NEXUS's data folder, so it was local-only, but SQLite is NEXUS's whole persistence layer. API change needed: schema version read as `i64`. All 174 tests pass, including migration and upgrade tests, and the app opened the user's existing database without a migration.
- Remaining notices (none has a fixed version):
  - `chrono` 0.4 unmaintained (RUSTSEC-2026-0543) — a **direct** dependency, used for timestamps. No known vulnerability. Recommendation: move to `jiff` or `time` when convenient.
  - `glib` 0.18 unsound `VariantStrIter` (RUSTSEC-2024-0429) — Linux only, via Tauri/wry's GTK stack; NEXUS never calls it. Fixed only in glib ≥ 0.20, which the GTK3 stack Tauri 2 uses cannot take.
  - `zbus_names`, `zvariant`, `zvariant_derive` unmaintained — Linux only, via `keyring`'s Secret Service backend.
  - `proc-macro-error` unmaintained — build time only.
- `npm audit`: 0 vulnerabilities, before and after (production and development).
- No dependency was added. One tool was installed locally for the audit (`cargo-audit`).
- CI now fails pull requests on any RustSec vulnerability or a high/critical npm advisory, and checks that both lockfiles match their manifests.

## Remaining risks

> Status after Security 2.0 (below): R2, R3 and R7 are resolved; R1 is mitigated where a sandbox backend exists. The current list is at the end of this document.

| ID | Sev | Risk | Mitigation / recommendation |
| --- | --- | --- | --- |
| R1 | P2 | Approved commands (tasks, agent tests) are not sandboxed: `npm test` in a hostile repository runs whatever its `package.json` says, with the user's rights and network. | By design and documented; approval shows the exact command. Recommend an opt-in sandbox (macOS `sandbox-exec` profile, Linux namespaces, or a container) for agent test commands. |
| R2 | P3 | The Developer prompt sends files unmasked (it must reproduce them). | Remote models already need explicit permission; recommend excluding files that contain detected secrets from the Developer context and telling the user. |
| R3 | P3 | Task commands, including AI-planned ones, receive the user's full environment (agent commands do not). | Recommend scrubbing for steps whose command came from AI planning, keeping the full environment for steps the user typed. |
| R4 | P3 | User-initiated commits run repository hooks; `git add -A` inside an agent worktree runs clean filters from the repository config. | Same as the Git CLI; documented. |
| R5 | P4 | `resolve_inside` canonicalises, then the caller opens: a local process swapping a path for a link in between could redirect one read. | Requires a local attacker already able to write the project; recommend `openat`-style handles if this ever matters. |
| R6 | P4 | `chrono` unmaintained (direct). | Plan a migration. |
| R7 | P4 | Commands have no time limit (they can be cancelled). | Consider an optional per-step timeout. |
| R10 | P3 | Prompt injection from repository text can steer agent plans and proposed commands. | Approvals, isolation and capability checks bound the effect; cannot be eliminated. |


---

# Security 2.0 — execution isolation and hardening (November 2026)

Follow-up to the audit above. It addresses remaining risks R1 (unsandboxed commands), R2 (Developer prompt unmasked), R3 (task environment) and R7 (no time limits), and reviews process lifecycle and resource limits across the app.

## Execution architecture

Approval decides **whether** a command runs; the execution policy decides **what it can touch** when it does. Every task step and agent command goes through one runner (`jobs::run_command`) with a `Policy` (`exec/mod.rs`):

| Part | Where | What it controls |
| --- | --- | --- |
| Level | `exec::Level` | Trusted, Restricted or Isolated |
| Backend | `exec/sandbox.rs` | How a level is enforced: the macOS sandbox (`sandbox-exec` with a generated Seatbelt profile), bubblewrap on Linux, or none |
| Confinement | `sandbox::confinement` | Writable folders, hidden paths, network on/off |
| Environment | `exec/env.rs` | Variables passed, built from scratch for every command |
| Limits | `Policy`, `jobs::apply_limits` | Time limit, output kept, largest file written (`RLIMIT_FSIZE`), memory (`RLIMIT_AS`, Linux Isolated), no core dumps |
| Record | first line of the output | The exact protection the command ran with, its time limit, and how many variables were withheld |

Backends are probed by running them once (`sandbox::backend`). A level that has no backend on the machine is **refused**, never run at a lower level: the settings screen marks it unavailable, saving it is rejected, and a stored setting that becomes unavailable falls back to the default for that field.

### What each level guarantees

| | Trusted | Restricted | Isolated |
| --- | --- | --- | --- |
| Runs as | the user | the user, inside a sandbox | the user, inside a sandbox |
| Writes | anywhere the user can | working folder, temporary folders, tool caches (`~/.cargo/registry`, `~/.npm`, `~/.cache`, `~/Library/Caches`, …) | working folder and temporary folders |
| Hidden (no read, no write) | nothing | SSH, cloud, Git and registry credentials (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.kube`, `~/.docker`, `~/.netrc`, `~/.git-credentials`, `~/.config/gh`, `~/.cargo/credentials*`, keyrings, …), NEXUS's data folder | as Restricted, plus package-manager settings (`~/.npmrc`, `~/.yarnrc*`, pip config) |
| Keychain / Secret Service | reachable | macOS: keychain services blocked. Linux: `$XDG_RUNTIME_DIR` (session bus) hidden | same |
| Network | yes | yes | macOS: loopback only (other programs on this computer reachable). Linux: none at all (own network namespace) |
| Environment | user's, minus credential-like variables | allow-list only (PATH, HOME, locale, toolchain homes, Windows system variables) | allow-list only |
| Variables the user allows | passed | passed | passed |
| File size limit | no | yes | yes |
| Memory limit | no | no | Linux only |
| Process namespace, dies with NEXUS | no | Linux only | Linux only |

Levels per platform: macOS all three; Linux all three when bubblewrap is installed and user namespaces work, else Trusted only; Windows Trusted only (no file-system isolation backend; see residual risks).

Defaults: task steps Trusted (commands the user wrote); agent commands Restricted where available, otherwise Trusted. The approval card for every command shows the level and time limit; Trusted is shown as "as you, not sandboxed".

## Findings and fixes

| ID | Sev | Finding | Fix | Tests |
| --- | --- | --- | --- | --- |
| EXEC-1 | P2 | Approved commands ran with the user's full file access (former R1) | Restricted and Isolated levels with platform backends | `restricted_commands_cannot_reach_hidden_data`, `writes_outside_the_writable_folders_are_refused`, `isolated_commands_reach_only_this_computer`, `levels_confine_as_described`, `profiles_quote_paths_safely` |
| ENV-1 | P2 | Task commands got the full environment, including credentials, also for AI-planned steps (former R3); the name check missed credential URLs under plain names, and matched `GIT_AUTHOR_NAME` | Environment built from scratch per level; segment-based name check plus value check (URLs with a password, vendor token formats); explicit per-variable allow-list | `levels_pass_what_they_promise`, `names_are_classified_by_segment`, `values_are_recognised_whatever_the_name`, `credentials_are_withheld_unless_allowed` |
| SEC-2 | P2 | The Developer prompt carried credentials verbatim (former R2) | Protected editing: `secrets::Vault` swaps each value for a per-run placeholder and restores it exactly in the files written; unknown or foreign placeholders are refused | `protect_then_restore_is_exact_and_hides_every_value`, `edited_output_gets_the_real_values_back`, `the_developer_never_sees_credentials_and_the_code_keeps_them`, `a_forged_placeholder_fails_the_step` |
| SEC-3 | P3 | Read-only masking was a few regexes | Structured detector (`secrets::detect`) shared by masking and the vault, with tests for false positives | `finds_each_kind_of_credential`, `references_placeholders_and_ordinary_code_are_not_secrets`, `masking_keeps_names_and_hides_values` |
| TIME-1 | P2 | No time limits (former R7) | Default limits for tasks and agent commands, per-task override, clear record; whole tree stopped | see Reliability |
| PROC-2 | P2 | Children that left the process group survived cancel; commands left by a crash ran on | Descendant walk on stop; crash records reaped at next start | see Reliability |
| NET-4 | — | Found during development, never shipped: a loopback rule `(allow network* (local ip "localhost:*"))` also allowed outbound connections | Only bind and inbound are allowed locally | `isolated_commands_reach_only_this_computer` |
| RISK-1 | P4 | `>/dev/null` was flagged "destructive" | Only writes to disk devices are flagged | `flags_risky_commands` |
| EXEC-2 | P3 | A Restricted task without a project would have had the whole home folder writable (found in the UI check) | Home itself is never made writable | `levels_confine_as_described` |

### Secret detection: what it finds and what it does not

Finds: private-key bodies; AWS, GitHub (classic and fine-grained), GitLab, Slack, OpenAI/Anthropic, Stripe, Google, Hugging Face and npm tokens; JWTs; passwords inside `scheme://user:password@host`; string values assigned to secret-named identifiers (`password`, `apiKey`, `client_secret`, `authToken`, also with a type annotation such as `password: &str = "…"`); `KEY=value` lines in `.env` files and shell scripts.

Leaves alone: environment references (`process.env.X`, `os.environ[...]`, `${X}`), placeholders (`your-api-key`, `changeme`, `<token>`), prose, short values, React-style `key` props, function calls (`read_password()`).

Does not find: a credential with no recognisable format under an ordinary name (`const x = "a8f…"`), credentials split across lines or built by concatenation, binary files. With the vault a false positive costs the model some context but never changes code; with masking it hides a harmless string.

## Remaining risks after Security 2.0

| ID | Sev | Risk | Notes |
| --- | --- | --- | --- |
| R1′ | P2 | Trusted commands (the default for task steps, and the only level on Windows) are not sandboxed | Stated on every approval card and in every command record |
| R8 | P3 | Restricted allows network: a command can upload what it can read (project files, non-hidden home files) | Use Isolated for untrusted code; it blocks the network |
| R9 | P3 | Restricted and Isolated can still **read** most of the home folder (documents, other projects) except the hidden credential paths | Deny-by-default reads break toolchains; a future level could run with a private HOME |
| R10 | P3 | macOS `sandbox-exec` is deprecated by Apple | Still enforced on macOS 27.2 (tested here); NEXUS probes it at start-up and refuses the levels if it stops working |
| R11 | P3 | Windows: no isolation backend, no file-size or memory limits | A restricted token or AppContainer backend is the natural next step |
| R12 | P3 | Secret detection is pattern-based (see above) | Unrecognised credentials reach the Developer and Ask prompts |
| R13 | P4 | macOS Isolated allows connections to other programs on this computer (loopback); Linux Isolated does not | Documented per platform |
| R14 | P4 | Terminals are the user's own interactive shell: full environment, no sandbox | Intentional; equivalent to opening Terminal.app |
