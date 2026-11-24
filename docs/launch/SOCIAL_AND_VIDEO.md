# Short videos, social posts, contributor call

Drafts for the maintainer to edit and post personally. Every video is a screen recording of the real app (0.2.0 or a build from `main`) on a throwaway data folder (`NEXUS_DATA_DIR`) and a public repository, so no private paths or code appear. No mock-ups, no sped-up results presented as real time (say "sped up" if it is).

## Five demo-video concepts

| # | Title | Length | What happens on screen | Why it works |
| --- | --- | --- | --- | --- |
| 1 | **"Map a repo you've never seen in 30 seconds"** | 30 s | Add a well-known public repository → index finishes → Intelligence → Map → hover a module to highlight what it imports → a red cycle | Shows the core value with no AI and no setup |
| 2 | **"Share a codebase without sharing the code"** | 45 s | Overview → Share a report → open the HTML file in a browser → scroll the graph, hotspots, dependencies → search the file for a known function body: not there | The one thing no comparable tool does |
| 3 | **"An agent that has to ask"** | 60–90 s | Agents → New with a local model and a small task (add a test) → plan gate → approve → a command waiting with "Runs Restricted (macOS sandbox)" → final diff → reject, show the project unchanged | Shows control rather than magic; rejecting is the punchline |
| 4 | **"Which local model is fastest on my laptop?"** | 45 s | Model Lab → pick two or three installed models → run the same benchmark → comparison table (your own real numbers) | Concrete, useful to r/LocalLLaMA readers, and true only for your machine — say so |
| 5 | **"A sandbox that refuses instead of pretending"** | 40 s | Settings → Execution → Restricted → a command that tries to read `~/.ssh` fails; on a system without the sandbox the step is refused, not downgraded | For security-minded developers; matches the technical article |

Record at 1280×800 or larger, no music needed, captions on (many people watch muted). Use [DEMO_SCRIPT.md](DEMO_SCRIPT.md) for the longer 90-second version.

## Social posts

For X, Bluesky or Mastodon. One link per post, one or two hashtags at most, attach the media named. Post two or three a week, not all at once.

1. **Launch** (tour GIF):
   > I built NEXUS, an open-source desktop app for understanding a codebase on your own machine: architecture map, dependency cycles and hotspots — no AI, no account, nothing uploaded. AI agents are optional and need your approval at every step. MIT, macOS/Windows/Linux. https://github.com/yazhiniyanMS/nexus

2. **The report** (screenshot of the report):
   > You can now share the shape of a private codebase without sharing the code: one HTML file with the module graph, hotspots, dependencies and findings. Paths and names, no source. A test checks that no source text ends up in it.

3. **A real finding** (map screenshot of NEXUS itself):
   > Mapped NEXUS with NEXUS: at three folder levels it's 16 modules and 29 dependencies, and there's a cycle between src, src/components and src/views. Not dangerous, but now I know it's there. What does your project's map look like?

4. **Engineering — sandboxing** (code snippet image):
   > A bubblewrap lesson: mounting a tmpfs over a folder to hide it doesn't stop writes — they land in a throwaway layer and the test still "passes". `--tmpfs dir --remount-ro dir` makes the write fail, which is what you actually want.

5. **Engineering — deadlock**:
   > Bug of the week in a Tauri app: holding a mutex while calling something that needs the main thread, while the main thread waited on the same mutex. The app froze only when a task step waited for approval. Write-up: [link to the technical article]

6. **Design choice**:
   > Most agent tools ask "allow this edit?" one file at a time. NEXUS makes an agent work in a separate Git worktree and asks three times: the plan, each command, the final diff — and the diff applies all-or-nothing. Slower. On purpose.

7. **Honesty post**:
   > Things NEXUS doesn't do (yet): it isn't an editor, its agents are weaker than Claude Code's, Windows has no sandbox, the installers aren't signed, and the index understands five languages. If one of those is a dealbreaker, I'd like to know which.

8. **Local models** (Model Lab screenshot, your numbers):
   > Benchmarked [model A] and [model B] on my [machine] with the same prompts: [your measured tokens/s and time to first token]. NEXUS's Model Lab runs this on your hardware, so the numbers are yours, not a leaderboard's.

9. **Asking for testers**:
   > Looking for a few people on Windows or Linux to try NEXUS for 20 minutes and tell me what broke. There's an issue form that asks exactly the right questions: https://github.com/yazhiniyanMS/nexus/issues/new?template=platform_report.yml

10. **Milestone, only when true** (release notes screenshot):
    > NEXUS 0.2.0 is out: the codebase report, a guided first run, sandboxed execution levels and a redesigned interface. Thanks to [names of real contributors or reporters, with their permission]. https://github.com/yazhiniyanMS/nexus/releases

## Contributor call

For the repository (pinned Discussion or README news), r/opensource, and Discord showcase channels:

> **NEXUS is looking for its first contributors.**
>
> NEXUS is an MIT-licensed desktop app (Rust + TypeScript, Tauri 2) that maps codebases without AI and runs AI agents only through approvals. It's been a one-person project, and there are well-defined places to help:
>
> - **20 minutes, no code:** try it on Windows or Linux and send a platform report.
> - **TypeScript:** remember the chosen map level per project.
> - **Rust:** teach the code index Java (declarations and imports), with tests modelled on the existing languages.
> - **Docs:** a hands-on install check of the Linux `.deb` on Ubuntu 24.04.
>
> Every issue and pull request gets a reply within a week, and contributors are credited in the release notes. Start here: https://github.com/yazhiniyanMS/nexus/blob/main/docs/CONTRIBUTING.md
