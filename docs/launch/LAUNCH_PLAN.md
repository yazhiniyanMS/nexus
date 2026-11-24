# Launch plan

Nothing here has been posted. Every post is a draft for the maintainer to edit and publish personally, after reading the community's current rules. No automation, no vote requests, no sock puppets, no cross-posting the same text everywhere.

## Before launching (blockers)

1. **Ship 0.2.0.** The download is 0.1.0, which lacks the report, guided setup, sandboxing and the new interface. Launch posts send people to the download, so release first: push, let CI pass on Windows and Linux, tag, review the draft release, publish ([RELEASES.md](../RELEASES.md)).
2. **Enable GitHub Pages** so the website is live, and add it as the repository homepage ([GITHUB_SETUP.md](GITHUB_SETUP.md)).
3. **Repository metadata**: description, topics, social preview, private vulnerability reporting, Discussions ([GITHUB_SETUP.md](GITHUB_SETUP.md)).
4. **Two or three real `good first issue`s** (candidates below), so people who want to help have somewhere to start.
5. Be available for the first day after each post to answer questions.

## Order

| Week | Where | Why there |
| --- | --- | --- |
| 1 | Your own blog or dev.to: [LAUNCH_POST.md](LAUNCH_POST.md) | A canonical page every other post can link to |
| 1 | Show HN | Developers who try new tools; it must be something people can install and use |
| 1–2 | r/LocalLLaMA | Local-model users are the most natural first audience; lead with Model Lab benchmarks and local agents |
| 2 | r/rust ("What's everyone working on" thread, or a post focused on the Rust engineering) | The core is Rust; the sandbox and process work is interesting there |
| 2 | r/opensource | A complete, MIT-licensed project looking for contributors |
| 2–3 | [TECHNICAL_ARTICLE.md](TECHNICAL_ARTICLE.md) on dev.to / Hashnode; submit to Lobsters only if you have an invite | Engineering readers; links back to the project naturally |
| 3 | Awesome lists by pull request: awesome-tauri, awesome-rust (Development tools), local-LLM tool lists | Long-term discovery; follow each list's contribution rules |
| 3 | This Week in Rust ("Project/Tooling Updates", by pull request) | One line, read by many Rust developers |
| Later | Product Hunt | Only once installers are signed; unsigned-app warnings hurt first impressions there |

## Drafts

### Show HN

**Title:** `Show HN: NEXUS – understand a codebase locally, then let agents change it only with approval`

**Text:**

> Hi HN, I'm a student and I built NEXUS, an open-source (MIT) desktop app for macOS, Windows and Linux.
>
> It does two things. First, it maps a local codebase without any AI: modules and the imports between them, dependency cycles, the most-imported files, symbols and references. It can also save a single HTML report of all that, with no source code in it, so you can share the shape of a private project.
>
> Second, it runs AI agents as proposals: the agent plans, writes in a separate Git worktree, runs tests and reviews; I approve the plan, each command and the final diff. Commands run sandboxed on macOS (Seatbelt) and Linux (bubblewrap) — and if a sandbox isn't available, NEXUS refuses rather than running unsandboxed. It works with Ollama and other local models.
>
> It's a beta built by one person: it's not an editor, the agents are much weaker than Claude Code's, and Windows/Linux have had little hands-on use. I'd really like to know whether the report is useful and whether the approval flow feels safe or just slow.
>
> https://github.com/yazhiniyanMS/nexus

Rules to check: [news.ycombinator.com/showhn.html](https://news.ycombinator.com/showhn.html). Post on a weekday morning US time; reply to comments; never ask anyone to upvote.

### r/LocalLLaMA

**Title:** `I built an open-source desktop app that benchmarks local models on your hardware and runs approval-gated coding agents with them`

> I've been using Ollama for coding help and wanted two things: a way to compare models on my own machine, and agents I could trust with a local model without them editing my files directly.
>
> NEXUS (MIT, Tauri + Rust) has a Model Lab that detects Ollama and llama.cpp servers, can start them bound to localhost, and runs repeatable benchmarks (time to first token, tokens/s, load time) across models with the same prompts. Its agents plan, write in a separate Git worktree and wait for approval at each step; test commands run sandboxed.
>
> Honest limits: small models (7B) often fail to produce valid plans; 14B+ works better. [Add your own measured results here before posting — do not post numbers you have not run.]
>
> https://github.com/yazhiniyanMS/nexus — feedback welcome, especially on which models work for you.

Check the subreddit's self-promotion rules first; contribute to other threads, not only your own.

### r/rust

**Title:** `NEXUS: a Tauri 2 desktop app with a Rust core for code intelligence and sandboxed, approval-gated agents`

> The interesting Rust bits: a line-based code index for five languages with module graphs and cycle detection; a process runner that applies a Seatbelt profile (macOS) or bubblewrap (Linux) per command, stops whole process trees including `setsid` escapees, and reaps commands left behind by a crash; Git worktrees for agent isolation; credentials swapped for placeholders before files go to a model. The write-up of what went wrong along the way (a sandbox rule that opened outbound networking, a mutex/main-thread deadlock): [link to the technical article]. MIT: https://github.com/yazhiniyanMS/nexus

### r/opensource

> NEXUS is an MIT-licensed desktop workspace for understanding codebases locally (architecture map, cycles, a shareable report with no code) and running AI changes as approval-gated proposals. It's a one-person beta and I'm looking for people to try it on Windows and Linux, and for contributors — language support for the index is a good self-contained place to start. https://github.com/yazhiniyanMS/nexus

### Short post (X, Bluesky, Mastodon)

> I built NEXUS: an open-source desktop app that maps any codebase without AI — modules, cycles, hotspots — and saves a shareable report with no source code in it. When you want AI help, agents work in a separate Git worktree and wait for your approval at every step. Works with local models. MIT. https://github.com/yazhiniyanMS/nexus

Attach `docs/assets/demo/nexus-tour.gif` or a screenshot. Hashtags: one or two at most.

## Good first issue candidates (real gaps, from the code and docs)

Open these as issues yourself if you agree; they are not filed yet.

1. **Index Java files** — add declaration and import extraction to `intel/extract.rs` and `intel/resolve.rs`, with tests like the existing ones.
2. **Report: show the date range of the analysis** in the header, and **Markdown export** of the same content.
3. **Map: remember the chosen level** per project (currently resets to two levels).
4. **Docs: Linux hands-on check** — install the `.deb` on Ubuntu 24.04 and update [LINUX.md](../LINUX.md) with what happened.

## What not to do

- Do not buy stars, upvotes or followers, use bots, or ask friends to upvote.
- Do not post the same text to several communities on the same day.
- Do not claim users, downloads or benchmarks you do not have.
- Do not argue with criticism; thank people and file the good points as issues.
