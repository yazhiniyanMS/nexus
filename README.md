# NEXUS

A local-first developer workspace for macOS, Windows and Linux. **Public beta (0.1.0).** Import your repositories, understand them, search them, inspect their Git state, run approved development tasks, and optionally bring an AI provider. Everything except AI requests you make stays on your machine, and the app is fully usable with no AI key.

Built with Tauri 2, React + TypeScript, Rust, SQLite, Tailwind CSS and Monaco.

## Features

| Area | What it does |
| --- | --- |
| **Projects** | Import local folders. Detects languages, frameworks and dependencies (npm, Cargo, pip, Go), summarises the codebase and flags findings: possible secrets, committed `.env` files, oversized files, TODO/FIXME markers, missing README/tests/CI. Browse files in a read-only Monaco viewer. |
| **Intelligence** | A code index for Rust, TypeScript, JavaScript, Python and Go. Interactive dependency graph and architecture map with cycle detection; ranked symbol search; reference search; per-file connections (imports, used by); source lookup for questions with optional AI answers that cite the files they used; generated architecture document. Works without an AI provider. |
| **Agents** | Describe an outcome; five specialised agents (Research, Architect, Developer, Testing, Review) carry it out under one orchestration engine. The plan, every command and the final diff each wait for your approval. Work happens in an isolated Git worktree, your uncommitted changes are never touched, and applying is all-or-nothing. Runs are queued, pausable, cancellable, resumable after a crash, and fully logged. Requires an AI provider. |
| **Terminal** | Your own shell inside each project, in real pseudo-terminals. Tabs, split panes you can drag to resize, process status and exit codes, restart, and search across saved output. The layout is saved per project; output is kept on disk so it survives leaving the screen or reloading. Only what you type is sent to a shell: agents, tasks and AI have no way to write to a terminal. |
| **Model Lab** | Experimental. Detects local runtimes (Ollama, llama.cpp servers, OpenAI-compatible endpoints) and can start Ollama and llama.cpp servers; lists models with the metadata they report; downloads models only when you ask (Ollama pull, Hugging Face GGUF files with checksum verification); runs prompts with streaming and measured timings; benchmarks and compares models on your hardware; manages connections and defaults used by Intelligence and Agents. See [docs/MODEL_LAB.md](docs/MODEL_LAB.md). |
| **Search** | File-name and content search (text or regex) across every project. Respects `.gitignore` and skips generated folders. |
| **Git** | Branch, upstream and ahead/behind; staged, changed and untracked files with diffs; commit history with patches; local and remote branches. Stage and unstage files; these only change the index and never touch files on disk. |
| **Tasks** | Tasks made of steps. Steps can be suggested from the project's manifests or planned by AI. **Nothing runs without approval**: each command waits for an explicit yes, destructive-looking commands are flagged, output streams live, and a run can be cancelled (the whole process tree is killed). |
| **AI Workspace** | Optional. Anthropic, OpenAI, any OpenAI-compatible endpoint, Ollama and LM Studio. API keys are stored in the OS keychain, never in the database. Shows token usage and timing. Detects local runtimes and lists their models. |
| **Activity** | Local log of imports, analyses, executed commands, task results and AI usage. |

## Install

Download an installer from the releases page: `.dmg` (macOS), `-setup.exe` or `.msi` (Windows), `.AppImage`, `.deb` or `.rpm` (Linux). The beta installers are unsigned. See [docs/INSTALLATION.md](docs/INSTALLATION.md) for requirements, first-launch warnings, updating and uninstalling.

| System | Status |
| --- | --- |
| macOS (Apple Silicon) | Reference platform, used daily in development. |
| macOS (Intel) | Compiled, packaged and launch-checked under Rosetta; never run on an Intel Mac. |
| Windows 10/11 x64 | New in the beta. See [docs/WINDOWS.md](docs/WINDOWS.md). |
| Linux x64 (Ubuntu 22.04) | New in the beta. See [docs/LINUX.md](docs/LINUX.md). |

Exactly what has been compiled, tested, installed and checked by a person on each system is in [docs/RELEASES.md](docs/RELEASES.md#verification-status).

## Navigation

| Shortcut | Action |
| --- | --- |
| `⌘K` (macOS) / `Ctrl+K` (Windows, Linux), or `Ctrl+Shift+P` anywhere | Command palette: screens, projects, actions and live symbol search |
| `⌘1…9` / `Ctrl+1…9` | Switch screen |
| `Esc` | Close the palette, the file drawer or a dialog |

On Windows and Linux, `Ctrl` combinations typed inside a terminal go to the shell; copy and paste there are `Ctrl+Shift+C` / `Ctrl+Shift+V`.

The project selected in the sidebar applies to every screen. **About & feedback** in the sidebar shows the version, system details and a diagnostic report you can review and copy into a bug report.

## Develop

Prerequisites: Node 24, Rust (version pinned in `rust-toolchain.toml`), Git, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev        # run the app
npm run tauri build      # build installers for this OS
cargo test --manifest-path src-tauri/Cargo.toml
npm test                 # frontend and release-script tests
```

More in [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md). Releases are described in [docs/RELEASES.md](docs/RELEASES.md).

## UI test bridge (debug builds only)

Debug builds started with `NEXUS_E2E_PORT=<port>` open a loopback-only socket that runs JavaScript inside the real window, so features can be tested through the actual interface. It is compiled out of release builds.

## Architecture

```
src/                 React UI
  lib/api.ts         the only place that calls the native core
  views/             one file per workspace area
src-tauri/src/       Rust core, one module per feature
  db.rs              SQLite + migrations
  projects.rs        import and analysis
  search.rs          cross-project search
  git.rs             Git status / log / diff (shells out to git)
  tasks.rs           task engine: approval, execution, cancellation
  agents/            agent orchestration, roles, isolated workspaces
  intel/             code index, graphs, retrieval
  terminal.rs        pseudo-terminal sessions, scrollback, layouts
  jobs.rs            shared process runner
  lab/               Model Lab: runtimes, registry, downloads, inference, benchmarks
  platform/          everything that differs between macOS, Windows and Linux
  settings.rs        stored settings
  ai.rs              providers, keychain, chat, usage
  activity.rs        activity log and stats
```

Data lives in one SQLite file in the OS app-data directory (`dev.nexus.workspace/nexus.db`).

## Privacy

No account, no analytics, no background network activity. What can leave your machine, and when, is listed in [docs/PRIVACY.md](docs/PRIVACY.md).

## Security model

- The webview has no filesystem or shell access; it can only call the typed commands in `lib.rs`.
- File reads are confined to imported project folders (paths are canonicalised and checked).
- Commands run only as task steps or agent test steps, only after the user approves that specific command, or because the user typed them into a terminal.
- A terminal is the user's own interactive shell and is not sandboxed. The only path into it is keyboard input and paste on the Terminal screen; no agent, task or AI code can send input to a terminal. A terminal starts inside the selected project and does not inherit NEXUS's stored API keys (they are never in the environment).
- Agents write only inside an isolated Git worktree; paths are confined and symlinks refused. Changes reach the project only when the user approves the exact diff.
- Model Lab never runs model output; it starts only Ollama and llama.cpp servers, from their own programs with fixed arguments and no shell, bound to 127.0.0.1. Project content goes to a connection that is not on this machine only after the user confirms it on that screen, and the core checks that confirmation.
- API keys live in the macOS Keychain, Windows Credential Manager or the Linux Secret Service.
- A strict Content Security Policy is applied; no remote scripts or CDNs are loaded.

## Known limits

- Agents need a Git repository with at least one commit. The Developer returns whole files and cannot delete files. Commands you approve are not network-sandboxed. Pause takes effect before the next step, not mid-command.
- Terminal: shells stop when NEXUS closes (their output is kept and shown as ended). Saved output is limited to about 1–2 MB per terminal and is replayed at the current pane width, so old full-screen or wrapped output can look different after a reload. Output search works on the raw stream, so lines the shell redrew can appear doubled. Not tested on Windows or Linux yet.
- Intelligence extracts symbols and imports with line-based rules, not a parser, so unusual constructs can be missed; references are matched by name, not by scope; Java, C#, C/C++, Kotlin and Swift are not indexed. See `docs/ARCHITECTURE.md`.
- The code index is rebuilt in full on request; there is no file watcher.
- Git supports staging and unstaging only (no committing, discarding or pushing from the app yet).
- AI Workspace chat replies are not streamed and its history is not saved between sessions (the Model Lab Playground streams and saves sessions).
- Model Lab: memory use and load time are reported only by Ollama; OpenAI-compatible endpoints report model names only; NEXUS can start Ollama and llama.cpp servers but does not install runtimes; Hugging Face downloads are limited to public, non-gated repositories.
- The AI chat receives a project summary, not file contents.
- Installers are unsigned; signing is prepared but needs certificates, and there is no auto-update.
- Windows and Linux are new and have had little hands-on use; only x64 is built there.
- Linux needs a running Secret Service (GNOME Keyring, KWallet) to store API keys.

The design of Agents, Terminal and Model Lab and the roadmap for Git Intelligence and Testing Lab are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

MIT
