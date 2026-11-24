# FAQ

**What is NEXUS, in one sentence?**
A local-first desktop app that maps any codebase without AI, and lets AI agents propose changes in an isolated copy that reach your files only after you approve the diff.

**Do I need an AI key or a model?**
No. Projects, analysis, the architecture map, symbols, search, the codebase report, Git, the terminal and tasks work with no model. Questions about the code and agents need one — a local model (Ollama, llama.cpp, LM Studio) needs no key.

**Is it free? What is the licence?**
Free and open source under the MIT licence.

**Which systems does it run on?**
macOS 12+ (Apple Silicon and Intel), Windows 10/11 x64 and Linux x64 (glibc 2.35+, WebKitGTK 4.1). Daily use is on Apple Silicon; the others are built and launch-checked in CI with little hands-on use so far. See [INSTALLATION.md](INSTALLATION.md).

**Does my code leave my computer?**
Only when you send something to a model that is not on your computer, and NEXUS asks you to confirm that for each request. There is no account, no telemetry, no analytics and no update check. Details: [PRIVACY.md](PRIVACY.md).

**Is the codebase report safe to share?**
It contains file paths, module and package names, counts and finding locations — no source code and no secrets' values. Check that the paths and names themselves are fine to share for your project.

**Can an agent damage my project?**
The agent writes only in a separate Git worktree. Your project changes only when you approve the final diff, which is applied all-or-nothing and refused if it clashes with your own edits. Test commands each need approval and run at the execution level you chose — sandboxed (Restricted, Isolated) on macOS and Linux with bubblewrap. A command you approve at Trusted runs as you, like typing it yourself.

**Is NEXUS an editor? Does it replace Cursor or Claude Code?**
No. Use NEXUS next to your editor. See [COMPARISON.md](COMPARISON.md) for what it does differently and where other tools are stronger.

**Which languages does the code index understand?**
Rust, TypeScript, JavaScript, Python and Go. Other languages still appear in analysis and search.

**Why are the installers unsigned?**
Signing needs paid certificates; it is on the [roadmap](ROADMAP.md). Each release has SHA-256 checksums to verify downloads.

**How do I report a bug or a security problem?**
Bugs: **About & feedback** in the app, or [GitHub issues](https://github.com/yazhiniyanMS/nexus/issues/new/choose). Security problems: privately, as described in [SECURITY.md](../SECURITY.md).
