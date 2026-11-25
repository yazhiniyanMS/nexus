# Changelog

## 0.2.1 — public beta (2026-11-25)

First public release of the 0.2 line. The `v0.2.0` tag was never released: its macOS build stopped on the agent worktree race fixed below.

### Added
- **Sharing without the code**: copy the architecture map as a Mermaid diagram (GitHub draws it in READMEs and issues); copy or save Model Lab benchmark results as a Markdown table (no prompts or server addresses); the report's footer links to the project.
- The map opens at the folder depth (one to three levels) that shows the most dependencies.
- Website with real screenshots and a screen recording; issue forms for platform reports and confusing screens.
- **Codebase report**: one self-contained HTML file with the module graph, cycles, hotspots, entry points, dependencies, findings and Git facts — and no source code. Overview → Share a report, or Intelligence → Docs.
- **Guided first run**: add and index a project, connect (or start) a local Ollama, then next steps. Every step can be skipped.
- **Execution levels** for approved commands — Trusted, Restricted (sandboxed) and Isolated (no network) — using the macOS sandbox or bubblewrap on Linux; refused rather than downgraded where unavailable. Settings → Execution.
- **Time limits** for task steps (per task too) and agent commands; the whole process tree is stopped on timeout, cancel and quit, and commands left by a crash are stopped at the next start.
- **Environment policy**: credentials are withheld from commands by name and by value; allow individual variables when a task needs them.
- **Protected editing**: credentials in files an agent edits are sent to the model as placeholders and restored only in the files written.
- **Commit from the Git screen**, merge and rebase state, conflict handling.
- Architecture map at three folder levels; the map now fits its labels.
- Settings screen; redesigned interface throughout.

### Changed
- Agent commands run Restricted by default where a sandbox exists.
- Git reads no longer run a repository's file-system monitor, external diff or text-conversion programs.
- API keys are never sent along a redirect to another server; oversized replies are refused.
- The database is backed up before an upgrade; a newer or damaged database is refused with a readable message.

### Fixed
- A possible freeze when a task step started waiting for approval.
- Path traversal in the untracked-file preview; agent workspaces left behind when a project was removed; commands running in the wrong folder when a project folder was gone.
- Commands writing `>/dev/null` were flagged as destructive.
- Two agent runs starting at the same moment in one project could fail with "failed to read …/commondir": Git worktree creation and cleanup are now serialised.
- Installation instructions for macOS 15 and later: right-click → Open no longer bypasses Gatekeeper; use Privacy & Security → Open Anyway.

### Known issues
- Installers are unsigned; no automatic updates.
- Windows has no sandbox backend (Trusted only); Windows and Linux have had little hands-on use.
- The index understands five languages with line-based rules.

See [docs/audit/](docs/audit/) for the full security and reliability record.


## 0.1.0 — public beta (2026-11-17)

First version intended for people outside the development machine.

### Added
- Windows and Linux support: a platform layer for shells, processes, paths, the command search path, program discovery, secure storage, disk space and notifications.
- Windows terminal through ConPTY with PowerShell 7, Windows PowerShell or Command Prompt; a shell menu for new terminals on every system.
- Installers for macOS (`.dmg`), Windows (NSIS `.exe`, `.msi`) and Linux (`.AppImage`, `.deb`, `.rpm`), with checksums and a release manifest.
- Pull-request, main-branch and tagged-release workflows on native macOS, Windows and Linux runners, including automated install and launch checks. Releases are created as drafts.
- First-run welcome screen; **About & feedback** with version, system details and a diagnostic report you review before sending.
- Notifications when a task or agent step is waiting for approval while the window is in the background.
- `NEXUS_DATA_DIR` to keep data in a folder of your choice.
- Model Lab: runtimes, model library, playground, benchmarks, comparison and provider settings (experimental).
- Terminal with tabs, splits and saved output; Agent Command Center; Project Intelligence.
- Public website and GitHub issue forms.

### Changed
- Visual identity: near-black surfaces with a restrained mint accent and a lavender accent for focus and intelligence features, defined once as design tokens.
- Approved commands and Git now use your login shell's `PATH`, so tools installed for your terminal are found when NEXUS is started from the desktop.
- Copy and paste in the terminal are `Ctrl+Shift+C` / `Ctrl+Shift+V` on Windows and Linux; `Ctrl` shortcuts typed in a terminal go to the shell.

### Known issues
- Installers are unsigned.
- Windows and Linux have not been used hands-on by the maintainers.
- No automatic updates; no committing or pushing from the Git screen.
