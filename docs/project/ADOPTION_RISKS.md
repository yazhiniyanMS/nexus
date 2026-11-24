# Top 10 reasons a new user would leave

November 2026. Ranked by how likely each is to make someone uninstall NEXUS or stop trusting it, with the evidence and the status. Updated as items are fixed.

| # | Risk | Evidence | Status |
| --- | --- | --- | --- |
| 1 | **The download is 0.1.0, which lacks everything the README and website show** (report, guided setup, sandboxing, new interface). Someone installs it and finds a different app. | Latest release is `v0.1.0` (17 Nov); the work is in PR #1. | **Open — the biggest one.** README and website now say so plainly. Fix: merge, release 0.2.0. |
| 2 | **Unsigned installers.** macOS Gatekeeper and Windows SmartScreen warn on first launch; many people stop there. | `release-manifest.json` marks every artifact unsigned. | Open. Needs paid certificates. Workaround documented in README, INSTALLATION, TROUBLESHOOTING and the website — **corrected 24 Nov**: since macOS 15 right-click → Open no longer works; the steps now say Privacy & Security → Open Anyway. |
| 3 | **AI features look empty without a model**, and most people have none set up. | On this machine Ollama was installed with an empty model store; Ask and Agents need a model. | Partly fixed: the first-run guide finds or starts Ollama and says when it has no models. The non-AI features are presented first everywhere. A one-click "download a small model" is not built (it would download gigabytes, so it must stay explicit). |
| 4 | **The map looked broken on nested projects** — one big circle at two levels. | NEXUS itself: 9 modules / 10 dependencies at two levels vs 16 / 29 at three. | **Fixed:** the map opens at the most informative depth (1–3); three levels added to the menu. |
| 5 | **Windows and Linux have had little hands-on use.** | CI builds, installs and launches them; no person has used them for real. | Open. CI now passes on both, including the Linux sandbox tests. The platform-report form asks users for exactly this. |
| 6 | **Agents with small local models fail validation**, which reads as "agents don't work". | Plans and file changes must be valid JSON; 7B models often fail. | Open. Documented in LIMITATIONS and FAQ. Roadmap: smaller, checkable steps for local models. |
| 7 | **No website** — the link people share is a GitHub page. | The `Website` workflow failed: GitHub Pages was never enabled. | **Fixed:** Pages enabled and the site is live at https://yazhiniyanms.github.io/nexus/ (24 Nov). The improved workflow is in PR #2. |
| 8 | **Windows commands are never sandboxed.** Security-minded users on Windows get only "Trusted". | `exec::sandbox::Backend::None` on Windows. | Open, stated in the app, README, website and LIMITATIONS. Roadmap item. |
| 9 | **The index covers five languages.** A Java or C# developer sees an empty map. | `intel/extract.rs` handles Rust, TS, JS, Python, Go. | Open. Good first contribution (Java). Search and analysis still work for all languages. |
| 10 | **No auto-update.** Fixes don't reach people who installed once. | No updater in the app. | Open. Needs signed builds first. Release notes and the GitHub "watch releases" option are the stopgap. |

## Fixed earlier in this release cycle

A freeze when a task step waited for approval; credentials following redirects; repository config running programs on `git status`; damaged databases ending start-up silently; commands running in the wrong folder; agent worktrees left behind. See [../audit/BUGS.md](../audit/BUGS.md).
