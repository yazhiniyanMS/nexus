# Product Hunt listing — ready to paste

Prepared 25 November 2026. **Not entered into Product Hunt yet:** no Product Hunt connection was available, and the browser was not signed in to Product Hunt. Paste each field into the existing "nexus" draft and save it as a draft. Nothing here is scheduled or published.

Every claim below is something NEXUS does in the code (verified in the code, tests or the running app). **One exception to check:** copying the map as Mermaid, the benchmark table and the report's footer link are in three commits on `prepare-0.2` that are not on `main` yet (PR #2 was merged before them). If 0.2.0 ships without them, delete the Mermaid sentence from the first comment. **Launch only after 0.2.0 is released**: the current download (0.1.0) lacks the codebase report, the sharing features and sandboxed execution levels.

Field limits used: tagline ≤ 60 characters; description ≤ 260 characters (Product Hunt's help pages and guides disagree between 260 and 500, so this fits both); up to 3 launch tags; gallery 1270×760; thumbnail 240×240; files under 3 MB.

## Basics

| Field | Value | Length |
| --- | --- | --- |
| Name | `NEXUS` | 5 |
| Tagline (as you asked) | `Your codebase. Fully understood.` | 32 |
| Tagline (recommended) | `Map any codebase locally. AI changes only with your approval` | 60 |
| Website | `https://yazhiniyanms.github.io/nexus/` | |
| Additional links | GitHub: `https://github.com/yazhiniyanMS/nexus` · Download: `https://github.com/yazhiniyanMS/nexus/releases` | |
| Pricing | Free (open source, MIT) | |
| Launch tags (3) | Developer Tools · Open Source · Artificial Intelligence | |
| Platforms | macOS, Windows, Linux (desktop) | |
| Open source | Yes — link the GitHub repository | |

**About the tagline:** "Fully understood" promises more than NEXUS does. The index reads five languages (Rust, TypeScript, JavaScript, Python, Go) with line-based rules, not a full parser, and Product Hunt commenters test claims like that. The recommended tagline says exactly what is true. If you keep yours, the description and first comment below keep the details honest.

## Description

**Your version (208 characters, accurate):**

> NEXUS is a local-first developer workspace that helps you understand your codebase, visualize architecture and dependencies, explore local AI models, and manage AI-assisted development with approval controls.

**Recommended (257 characters, more specific):**

> A free, open-source desktop app that maps any codebase on your machine — modules, dependency cycles, hotspots — with no AI or account. Share a report with no source code, benchmark local models, and let AI agents change code only after you approve the diff.

## Longer description (if the form offers 500 characters, or for the website field "about")

> NEXUS is a desktop app for macOS, Windows and Linux that helps you understand a codebase before anything changes it. It indexes a folder locally and draws the architecture — modules, imports, dependency cycles, most-imported files — with no AI, account or network. Save a one-file HTML report with no source code to share a private project's shape. When you want AI help, agents work in a separate Git worktree and wait for your approval of the plan, each command and the final diff.

(483 characters.)

## Maker's first comment

> Hi Product Hunt! I'm [your name], a student, and I built NEXUS because I spend more time understanding code I didn't write than writing new code — and the AI tools I tried started changing things before I knew what the project looked like.
>
> **What it does today**
> • **Architecture map without AI.** Point it at a folder; it indexes Rust, TypeScript, JavaScript, Python and Go locally and draws modules, imports and dependency cycles. Copy the map as a Mermaid diagram that GitHub renders in READMEs and issues.
> • **A report with no code in it.** One HTML file — graph, hotspots, dependencies, findings — that you can send someone before they have access to the repository. A test checks that no source text ends up in it.
> • **Agents that have to ask.** An agent plans, works in a separate Git worktree, runs tests and reviews its own diff; you approve the plan, every command and the final diff. Commands can run sandboxed on macOS and Linux.
> • **Local models, measured.** Model Lab benchmarks Ollama, llama.cpp or any OpenAI-compatible server on your own hardware.
>
> **Honest limits:** it's a beta built by one person. It isn't an editor. The agents are weaker than Claude Code's, and small local models often fail to make a valid plan. Windows has no sandbox yet, the installers aren't code-signed, and Windows and Linux have had little hands-on use.
>
> It's free and MIT-licensed. I'd love to hear what the map shows you about your own project — and if you're on Windows or Linux, a platform report on GitHub helps more than anything.

Fill in your name, or drop it and the word "student" if you prefer.

## Gallery (in this order)

All are real screenshots of NEXUS analysing its own repository (`docs/assets/screenshots/`), scaled to 1270×760 without cropping. Files are in this folder.

| # | File | Caption |
| --- | --- | --- |
| 1 | `gallery-1-architecture-map.png` | The architecture map: modules, imports and dependency cycles, built locally without AI |
| 2 | `gallery-2-report.png` | A shareable codebase report — graph, hotspots, dependencies — with no source code |
| 3 | `gallery-3-overview.png` | One project at a glance: repository state, findings and recent work |
| 4 | `gallery-4-tasks.png` | Every command waits for approval and says how it will run |
| 5 | `gallery-5-execution-settings.png` | Trusted, Restricted (sandboxed) or Isolated — described as enforced on your machine |
| 6 | `gallery-6-model-lab.png` | Model Lab: runtimes, models and benchmarks on your own hardware |
| 7 | `gallery-7-git.png` | Review changes, stage and commit |

**Thumbnail:** `thumbnail-240.png` (the app icon at 240×240). The animated tour `docs/assets/demo/nexus-tour.gif` can be used instead if the form accepts a GIF thumbnail under 3 MB (check its size first).

## Still needed from you

- **Sign in and paste** these fields into the "nexus" draft; confirm the form's description limit.
- **Maker profile:** your own Product Hunt account — add a photo and a one-line bio if empty.
- **Video (optional but recommended):** record the [60-second script](../DEMO_SCRIPT.md#60-second-version-youtube-shorts-x-the-readme) on 0.2.0 and add the YouTube link. No real recording exists yet; the tour GIF is a slideshow of screenshots, not a video.
- **Screenshots of the new sharing features** (Copy as Mermaid, benchmark table) once 0.2.0 is out — current screenshots predate them.
- **0.2.0 released** before the launch date.

## Launch date (recommendation — not scheduled)

I could not see the dates the form offers, because Product Hunt wasn't accessible. Recommendation:

**Tuesday 12 January 2027** (fallback: Wednesday 13 January). Product Hunt's day starts at 12:01 a.m. Pacific, which is **1:31 p.m. India Standard Time** on the same date in January. Be awake and replying for the first 6–8 hours, until about 9:30 p.m. IST.

Why then:
- **0.2.0 has to be out first**, plus a few weeks of real use ([CONTENT_CALENDAR.md](../CONTENT_CALENDAR.md) week 1–3), so installation problems are found by friends, not by Product Hunt visitors.
- **Avoid** US Thanksgiving week (26 November 2026) and 21 December – 4 January, when much of the audience is away.
- Tuesday to Thursday has the most visitors and the most competition. A **weekend** (for example Saturday 16 January 2027) has less of both, often better for a small open-source tool whose aim is feedback rather than a ranking. Either is fine; pick a day you can be present.
- **Unsigned installers** will show macOS and Windows warnings to Product Hunt visitors. If signing isn't possible by then, keep the first-comment line about it, and link the [installation guide](../../INSTALLATION.md) in your first reply.

When you have chosen, schedule it yourself in the Product Hunt form — nothing has been scheduled.
