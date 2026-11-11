# NEXUS

A local-first developer workspace for macOS, Windows and Linux. Import your repositories, understand them, search them, inspect their Git state, run approved development tasks, and optionally bring an AI provider. Everything except AI requests you make stays on your machine, and the app is fully usable with no AI key.

Built with Tauri 2, React + TypeScript, Rust, SQLite, Tailwind CSS and Monaco.

## Features

| Area | What it does |
| --- | --- |
| **Projects** | Import local folders. Detects languages, frameworks and dependencies (npm, Cargo, pip, Go), summarises the codebase and flags findings: possible secrets, committed `.env` files, oversized files, TODO/FIXME markers, missing README/tests/CI. Browse files in a read-only Monaco viewer. |
| **Search** | Text or regex search across every project. Respects `.gitignore` and skips generated folders. |
| **Git** | Branch, upstream and ahead/behind; staged, changed and untracked files with diffs; commit history with patches; local and remote branches. Read-only. |
| **Tasks** | Tasks made of steps. Steps can be suggested from the project's manifests or planned by AI. **Nothing runs without approval**: each command waits for an explicit yes, destructive-looking commands are flagged, output streams live, and a run can be cancelled (the whole process tree is killed). |
| **AI Workspace** | Optional. Anthropic, OpenAI, any OpenAI-compatible endpoint, Ollama and LM Studio. API keys are stored in the OS keychain, never in the database. Shows token usage and timing. Detects local runtimes and lists their models. |
| **Activity** | Local log of imports, analyses, executed commands, task results and AI usage. |

## Develop

Prerequisites: Node 20+, Rust stable, Git, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev        # run the app
npm run tauri build      # build installers for this OS
cargo test --manifest-path src-tauri/Cargo.toml
```

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
  ai.rs              providers, keychain, chat, usage
  activity.rs        activity log and stats
```

Data lives in one SQLite file in the OS app-data directory (`dev.nexus.workspace/nexus.db`).

## Security model

- The webview has no filesystem or shell access; it can only call the typed commands in `lib.rs`.
- File reads are confined to imported project folders (paths are canonicalised and checked).
- Commands run only as task steps, only after the user approves that specific step.
- API keys live in the macOS Keychain, Windows Credential Manager or the Linux Secret Service.
- A strict Content Security Policy is applied; no remote scripts or CDNs are loaded.

## Known limits

- Git is read-only (no staging, committing or pushing from the app yet).
- AI replies are not streamed, and chat history is not saved between sessions.
- The AI chat receives a project summary, not file contents.
- Installers are unsigned; code signing and auto-update are not configured.
- Linux needs a running Secret Service (GNOME Keyring, KWallet) to store API keys.

## License

MIT
