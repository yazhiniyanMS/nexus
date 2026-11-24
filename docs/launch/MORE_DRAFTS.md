# More drafts: Product Hunt, LinkedIn, short introduction

Drafts for the maintainer to edit and post personally. Nothing is posted automatically. Product Hunt is best left until installers are signed (see [LAUNCH_PLAN.md](LAUNCH_PLAN.md)).

## Product Hunt

**Name:** NEXUS
**Tagline (60 characters max):** Understand a codebase locally, change it only through you

**Description (260 characters max):**
> A free, open-source desktop app that maps any codebase without AI — modules, cycles, hotspots, a shareable report with no source code — and runs AI agents as proposals you approve step by step, in an isolated copy. Works with local models.

**First comment (maker):**
> Hi! I'm a student and I built NEXUS because I spend more time understanding code I didn't write than writing new code, and the AI tools I tried started changing things before I knew what the project looked like.
>
> Two things I'd love you to try: the architecture map (no AI, no key — it's built from a local index), and "Share a report", which saves an HTML file describing the project's structure without any source code.
>
> Honest limits: it's a beta made by one person, it isn't an editor, the agents are weaker than Claude Code's, Windows has no sandbox yet and the installers aren't signed. Feedback — especially from Windows and Linux — is very welcome: https://github.com/yazhiniyanMS/nexus

**Gallery:** `docs/assets/social-preview.png`, then `docs/assets/screenshots/architecture-map.png`, `report.png`, `tasks.png`, `execution-settings.png`.
**Topics:** Developer Tools, Open Source, Artificial Intelligence.

## LinkedIn

> I've been building an open-source desktop app called NEXUS, and it's ready for other people to try.
>
> The idea: before an AI tool changes your code, you should be able to see the code's shape yourself. NEXUS maps any local project without AI — modules, dependency cycles, the most-imported files — and can save a report of that structure with no source code in it, so you can share it.
>
> When you do want AI help, agents work in a separate copy of the project. I approve the plan, every command they want to run, and the final diff before anything touches my files. It works with local models through Ollama.
>
> Building it taught me a lot about Rust, Tauri, process sandboxing on macOS and Linux, and how many security assumptions a "simple" developer tool quietly makes. I wrote up the engineering here: [link to the technical article].
>
> It's MIT-licensed and a beta. If you try it — especially on Windows or Linux — I'd really value your feedback: https://github.com/yazhiniyanMS/nexus

## Short introduction (Discord, Slack, forum "show your project" threads)

> **NEXUS** — open-source desktop app (macOS/Windows/Linux) to understand a codebase locally: architecture map, dependency cycles and hotspots without any AI, plus a shareable report with no source code. AI agents are optional and work as approval-gated proposals in an isolated Git worktree; works with Ollama. Beta, built by a student, feedback very welcome: https://github.com/yazhiniyanMS/nexus

## Dev.to

Publish [LAUNCH_POST.md](LAUNCH_POST.md) as the announcement and [TECHNICAL_ARTICLE.md](TECHNICAL_ARTICLE.md) a week later. Tags: `opensource`, `rust`, `tooling`, `ai`. Set the canonical URL to your own blog if you post there first.
