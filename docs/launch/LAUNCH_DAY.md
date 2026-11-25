# Launch day — 25 November 2026 (NEXUS 0.2.1)

Final posts with live links. Post each one yourself, from your own accounts. No vote requests, no cross-posting the same text on the same day, reply to every comment.

**Links**
- Repository: https://github.com/yazhiniyanMS/nexus
- Release: https://github.com/yazhiniyanMS/nexus/releases/tag/v0.2.1
- Mac (Apple Silicon) download: https://github.com/yazhiniyanMS/nexus/releases/latest/download/NEXUS_0.2.1_aarch64.dmg
- Website and 50-second recording: https://yazhiniyanms.github.io/nexus/#demo

## Order for today (times IST)

| When | Where | Status |
| --- | --- | --- |
| 13:31 | Product Hunt goes live (00:01 Pacific) | Draft complete; **submit needs your confirmation** |
| 13:45 | X launch post + LinkedIn | X: paste below. LinkedIn: draft saved in Typefully |
| 19:30–20:30 | Show HN (weekday morning US Eastern) | Paste below |
| Tomorrow | r/LocalLLaMA; later in the week r/rust, r/opensource | Read each sidebar first — rules could not be checked automatically |

## Show HN

**Title** (80 characters max): `Show HN: NEXUS – Map a codebase locally; AI agents change it only with approval` (79 characters)

**URL:** `https://github.com/yazhiniyanMS/nexus`

**Text** (first comment):

> I'm a student and I built NEXUS, an open-source (MIT) desktop app for macOS, Windows and Linux (Tauri 2, Rust core, React UI).
>
> It does two things. First, it maps a local codebase without any AI: a line-based index for Rust, TypeScript/JavaScript, Python and Go, import resolution per language, module graphs at one to three folder levels, and dependency cycles (Tarjan). It can save one HTML report of all that with no source code in it, or copy the map as Mermaid for a README.
>
> Second, it runs AI agents as proposals: the agent plans, writes in a separate Git worktree, runs tests and reviews its diff; you approve the plan, each command and the final diff. Commands run Restricted (macOS Seatbelt / bubblewrap on Linux: writes only to the project, credential folders hidden) or Isolated (no network) — and if the sandbox is unavailable it refuses instead of running unsandboxed. Works with Ollama and other local models.
>
> Honest limits: one-person beta, not an editor, agents are much weaker than Claude Code's and small local models often fail to plan, Windows has no sandbox yet, installers are unsigned. I'd like to know whether the map and report are useful on your code, and whether the approval flow feels safe or just slow.

Rules: https://news.ycombinator.com/showhn.html — something people can try now, no asking for upvotes.

## Reddit

Subreddit rules could **not** be read automatically today (reddit.com blocks automated access). Read the sidebar and pinned posts of each before posting; if self-promotion isn't allowed, don't post there.

### r/LocalLLaMA

**Title:** `I built an open-source desktop app that benchmarks local models on your hardware and runs approval-gated coding agents with them`

> I use Ollama for coding help and wanted two things: a way to compare models on my own machine, and agents I could trust with a local model without them editing my files directly.
>
> NEXUS (MIT, Tauri + Rust) has a Model Lab that detects Ollama and llama.cpp servers, can start them bound to localhost, and runs repeatable benchmarks (time to first token, tokens/s, load time) across models with the same prompts — and you can copy the results as a Markdown table. Its agents plan, write in a separate Git worktree and wait for approval at each step; commands run sandboxed on macOS/Linux.
>
> Honest limits: small models (~7B) often fail to produce valid plans; 14B+ works better. [Add a table of your own Model Lab results here — Compare → Copy table — before posting.]
>
> 0.2.1 is out today: https://github.com/yazhiniyanMS/nexus — which models work for you?

### r/rust

**Title:** `NEXUS 0.2: a Tauri 2 app with a Rust core for code intelligence and sandboxed, approval-gated agents`

> The Rust parts that were interesting to build: a line-based code index for five languages with module graphs and iterative Tarjan cycle detection; a process runner that applies a Seatbelt profile (macOS) or bubblewrap (Linux) per command — including a bug where a tmpfs over a hidden folder let writes "succeed" into a throwaway layer until `--remount-ro` made them fail; whole-process-tree cleanup including `setsid` escapees; Git worktrees for agent isolation. MIT, feedback on the approach welcome: https://github.com/yazhiniyanMS/nexus

### r/opensource

> NEXUS 0.2.1 is out: an MIT-licensed desktop app for understanding codebases locally (architecture map, cycles, a shareable report with no code) and running AI changes as approval-gated proposals. It's a one-person beta and I'm looking for people to try it on Windows and Linux, and for contributors — there are four good first issues, from a TypeScript-only one to Java support in the indexer: https://github.com/yazhiniyanMS/nexus/labels/good%20first%20issue

## X

X isn't connected to the Typefully account, so paste these.

1. **Launch** (attach the demo video `launch-assets/nexus-demo-1080p.mp4` or the 720p `docs/assets/demo/nexus-demo.mp4`):
   > NEXUS 0.2.1 is out — an open-source desktop app that maps any codebase on your machine (modules, dependency cycles, hotspots) with no AI and no account. AI agents are optional and need your approval for the plan, every command and the diff. macOS, Windows, Linux. MIT.
   > https://github.com/yazhiniyanMS/nexus

2. **Demo** (attach the map poster `launch-assets/nexus-demo-poster-map.png`):
   > Point NEXUS at a folder and it draws the architecture: which modules import which, and the cycles you didn't know you had. Then copy it as Mermaid and GitHub renders it in your README. 50-second recording: https://yazhiniyanms.github.io/nexus/#demo

3. **Technical**:
   > A bubblewrap lesson from building NEXUS: mounting a tmpfs over a folder to hide it doesn't stop writes — they land in a throwaway layer and your test still "passes". `--tmpfs dir --remount-ro dir` makes the write fail, which is what you want. Sandboxing write-up in the repo.

4. **Follow-up (tomorrow)**:
   > Day one of NEXUS 0.2.1: thanks to everyone who tried it. The most useful thing you can send me now is a platform report from Windows or Linux — there's a form that asks exactly the right questions: https://github.com/yazhiniyanMS/nexus/issues/new?template=platform_report.yml

## Product Hunt

Listing: [producthunt/LISTING.md](producthunt/LISTING.md). The draft holds every field, the thumbnail, seven gallery images and the first comment, with launch day 25 November. Submitting it is the step that makes it public.
