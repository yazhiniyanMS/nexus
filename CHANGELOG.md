# Changelog

## 0.1.0 — public beta (unreleased)

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
