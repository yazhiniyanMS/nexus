# NEXUS threat model

Written from the code as it stands after the November 2026 audit (`src-tauri/src`, `src/`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/`). It describes what NEXUS actually does, not a generic desktop app.

## What NEXUS is, in security terms

A single-user desktop app. One Rust process owns a SQLite database, the user's API keys (through the OS keychain), pseudo-terminals, child processes, Git, and HTTP clients. One webview renders React and can only reach the Rust core through the Tauri commands registered in `lib.rs` (about 130). There is no server, no account and no listening network port in release builds.

## Assets

| Asset | Where it lives | Why it matters |
| --- | --- | --- |
| The user's source code and uncommitted work | Imported project folders | Loss or silent modification is the worst outcome |
| The user's shell and machine | Terminals, task and agent commands | Anything that runs a command runs as the user |
| API keys for model providers | macOS Keychain / Windows Credential Manager / Secret Service | Billing and data exposure |
| Secrets inside projects (`.env`, hard-coded keys) | Project files | Can leak through prompts to remote models |
| Other credentials in the environment (`GITHUB_TOKEN`, cloud keys) | Process environment | Inherited by child processes |
| NEXUS's own state | `nexus.db` (projects, tasks, agent runs and transcripts, activity, settings) | Corruption loses history, approvals and settings |
| Model files | Model folder | Large downloads; deletion is irreversible |

## Trust boundaries

| # | Boundary | Trusted side | Untrusted input crossing it | Controls in place |
| --- | --- | --- | --- | --- |
| 1 | React frontend | Code shipped in the app | Text from projects, models, providers, terminals rendered in the DOM | No `dangerouslySetInnerHTML`/`innerHTML`/`eval` anywhere; strict CSP (`script-src 'self'`, no remote origins); output rendered as text, terminal through xterm.js |
| 2 | Tauri IPC | Rust core | Every command argument | Capabilities grant only `core:default` and `dialog:default`; no shell, fs or opener plugin permissions to the webview; each command validates its own input (paths, hashes, names, URLs) |
| 3 | Rust backend | — | — | The privileged component; everything below is enforced here, not in the UI |
| 4 | Agent orchestration | The engine | Model output (plans, file contents, commands), repository text fed to models | Capabilities checked per role in the backend (`roles::require`); approvals bound to a step id and checked against the database; isolated Git worktree; paths confined (`safe_join`); file count and size caps; all-or-nothing apply of a patch computed before approval |
| 5 | Shell and terminal execution | The user | Commands proposed by AI or discovered from manifests | Nothing runs without an explicit approval of that exact command; destructive-looking commands flagged. Since Security 2.0: each command runs at a level (Trusted / Restricted / Isolated; agent commands Restricted by default where a sandbox exists), with an environment built per level, a time limit, output and file-size limits; the whole process tree is stopped on timeout, cancel and quit, and reaped after a crash |
| 6 | Filesystem | Project folders, NEXUS data folder | Paths from the UI, from models, from Hugging Face | `resolve_inside` (canonicalise + containment) for UI paths; `safe_join` for model paths; `storage::safe_name` allow-list for model files; walkers never follow links |
| 7 | Git repositories | The user's Git | The repository's own `.git/config`, hooks and attributes | NEXUS-initiated Git runs with `core.fsmonitor=false`, `--no-ext-diff --no-textconv`, no prompts; agent worktrees created with hooks disabled |
| 8 | AI provider APIs | — | Responses, error bodies, redirects | Same-origin redirects only; bounded bodies; keys redacted from errors; remote providers need explicit per-request permission for project content |
| 9 | Local model runtimes | — | Same as 8, plus NEXUS starts the process | Fixed binaries, argument arrays (no shell), loopback bind, allow-listed model files, scrubbed environment, capped count |
| 10 | SQLite | NEXUS | A database from another version or a damaged file | Schema version check (newer refused), backup before migration, per-step transactional migrations, readable start-up error |
| 11 | Project indexing | — | Arbitrary files: huge, binary, malformed, links, loops | Size cap (1.5 MB text, 64 MB search), binary detection, `.gitignore` honoured, generated folders skipped, links not followed, only source files of supported languages indexed |
| 12 | Plugins / extensions | — | — | None exist. `tauri-plugin-opener` and `-notification` are used only from Rust with fixed inputs |
| 13 | Imported project files | — | All content | Treated as untrusted everywhere above; masked before read-only prompts (`mask_secrets`) |
| 14 | External URLs and provider responses | — | URLs from the UI, Hugging Face API data, redirects | Downloads: HTTPS only (loopback allowed for tests), redirects must stay HTTPS, SHA-256 verified when published; feedback pages: the webview names a kind, never a URL |

## Attackers considered

1. **A malicious or careless repository** (cloned, downloaded as an archive, received on a shared drive). Controls files, `README`s, comments, build scripts, and possibly `.git/config` and hooks when the archive includes `.git`. This is the most realistic attacker.
2. **A model** (local or remote) returning hostile output: crafted plans, file contents aimed at paths outside the workspace, destructive commands, prompt-injected instructions.
3. **A provider endpoint** that is wrong, compromised or malicious: oversized or malformed replies, redirects.
4. **Another local process** running as the same user. Out of scope for prevention (it can already read the keychain-unlocked session and the database), but NEXUS must not make it easier (the UI test bridge exists only in debug builds and binds loopback).
5. **A network attacker** between NEXUS and a provider: mitigated by HTTPS via rustls and by refusing cross-origin redirects with credentials.

Not considered: a compromised OS, a malicious NEXUS build, physical access.

## Security assumptions

- The frontend code is trusted; a webview compromise (XSS) would give access to every IPC command, including terminal input. This is why the absence of HTML-injection sinks and the strict CSP are load-bearing.
- The user reads approval prompts. Approval is the control for command execution; NEXUS cannot judge whether `npm test` in a hostile repository is safe (its `package.json` decides what runs).
- Model output is never trusted to be correct or benign.
- The OS keychain protects keys at rest.

## If a component is compromised

| Compromised | Consequence | Containment |
| --- | --- | --- |
| Webview (XSS) | Full IPC: read project files, write to terminals (= run commands), start agent tasks | CSP blocks remote script and inline script in production; no HTML sinks |
| A model | Proposes harmful commands or changes | Each command and the final patch need approval; writes confined to an isolated worktree; size and count caps |
| A provider endpoint | Sees prompts (including masked excerpts) and the key sent to it | Key never sent to another origin; replies bounded |
| A repository | Runs code when the user approves a command it controls (`npm test` → its scripts); influences model output | Approval with the exact command shown; at Restricted (agent default) the code cannot write outside the workspace, temp and caches, cannot read credential folders, NEXUS's data or the keychain, and gets an allow-listed environment; at Isolated it also has no network. At Trusted (task default, and the only level on Windows) it runs as the user. NEXUS no longer runs repository-configured helpers or hooks on its own |
| A model provider (Developer step) | Would see credentials in the files it edits | Protected editing: it sees per-run placeholders; the real values are restored only in the files written locally |
| The database file | Loss of history, tasks and settings; replayed approvals are not possible (approvals are live channel messages checked against current step status) | Backup before migration; refusal of newer schemas; damaged files left untouched |
