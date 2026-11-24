# 30-day content calendar

Each piece should be useful to someone who never installs NEXUS. Write from real work: code, numbers you measured, mistakes you made. One substantial post a week, small posts between. Skip a day rather than post filler.

Start the calendar after 0.2.0 is released and the website is live.

| Day | Where | Piece | What the reader gets |
| --- | --- | --- | --- |
| 1 | Blog + dev.to | [Launch post](LAUNCH_POST.md) | Why the tool exists; what it does today |
| 2 | Show HN | [Draft](LAUNCH_PLAN.md#show-hn) | — |
| 3 | X / Bluesky / Mastodon | The architecture map of a well-known open-source repo (yours, or one you have permission to show) with one observation about its structure | A way to read a codebase's shape |
| 5 | r/LocalLLaMA | [Draft](LAUNCH_PLAN.md#rlocalllama), with **your own measured** Model Lab numbers for two or three models on your Mac | Real local-model numbers on known hardware |
| 7 | Blog | **How NEXUS analyses a repository without AI**: walking with `.gitignore`, line-based symbol extraction, resolving imports per language, and where line-based rules fail | A practical recipe for codebase indexing |
| 9 | Short post | A dependency cycle the map found in NEXUS itself (`src ↔ src/components ↔ src/views`) and whether it matters | Why cycles are worth seeing |
| 10 | r/rust | [Draft](LAUNCH_PLAN.md#rrust) | — |
| 12 | Short post | The deadlock: a mutex held across a main-thread call (from the technical article) | A bug pattern to avoid in Tauri apps |
| 14 | Blog + dev.to | [Technical article](TECHNICAL_ARTICLE.md): sandboxes, worktrees, process trees | Concrete Seatbelt/bubblewrap lessons |
| 16 | Short post | One sandbox rule that quietly opened all outbound networking, and the test that caught it | How to test a sandbox, not just configure it |
| 17 | Video (YouTube/X) | 90-second demo ([script](DEMO_SCRIPT.md)) | See the flow end to end |
| 19 | Short post | What the codebase report deliberately leaves out, and how a test proves it | Designing "safe to share" outputs |
| 21 | Blog | **Why AI agents need approval gates — and why "allow this edit?" is not one**: plan / command / diff gates, worktrees, refusing instead of downgrading | A design argument with code, usable in any agent tool |
| 23 | Short post | Building NEXUS with NEXUS: the report of its own repository, before and after a refactor | A before/after reading of a real codebase |
| 24 | r/opensource | [Draft](LAUNCH_PLAN.md#ropensource), asking for Windows/Linux testers | — |
| 26 | Short post | A platform report from a real user (with permission), or what CI caught on Windows that a Mac could not | Cross-platform pitfalls |
| 28 | Blog | **What worked and what failed in a month of building in public**: real numbers (issues, contributors, installs if known), what people asked for, what you changed | Honest open-source building notes |
| 30 | Newsletter / awesome lists | PRs to awesome lists; one line for This Week in Rust | Long-term discovery |

## Rules

- Never post numbers you did not measure. "Benchmarks" means your machine, your prompts, stated.
- Show other people's code only if it is public and you link it; show private code never.
- Reply to every comment in the first day; turn good criticism into issues.
