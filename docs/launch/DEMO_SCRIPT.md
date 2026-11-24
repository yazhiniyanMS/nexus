# Demo video script (about 90 seconds)

Record the real app, with real projects, at 1320×840 or larger. Use a throwaway data folder so nothing personal appears:

```bash
NEXUS_DATA_DIR="$HOME/nexus-demo-data" /Applications/NEXUS.app/Contents/MacOS/nexus
```

Before recording: set a neutral shell prompt in any terminal you show, close notifications, and make sure no API key, private repository or personal path is on screen. Use a public repository (NEXUS itself works well).

| Time | Show | Say (voice-over or captions) |
| --- | --- | --- |
| 0:00 | Welcome screen | "NEXUS: understand a codebase on your own machine, then change it only through you." |
| 0:06 | Choose a folder; analysis and index finish | "Point it at any project. It analyses and indexes it locally — no AI, no account." |
| 0:15 | Intelligence → Map, three levels; hover a red cycle | "Here's the architecture: modules, imports, and the dependency cycles in red." |
| 0:28 | Overview → Share a report; the HTML opens in a browser; scroll | "One click makes a report you can share — the graph, hotspots, dependencies — and no source code." |
| 0:42 | Git: a diff, stage, ⌘Enter commit | "Review what changed and commit." |
| 0:50 | Agents → New task with a local model; the plan gate | "When you want AI help, an agent plans first. You approve the plan…" |
| 1:02 | Command step: "Runs Restricted (macOS sandbox)" → Approve | "…each command — sandboxed, with a time limit…" |
| 1:12 | Apply gate showing the diff | "…and the final diff. Nothing touches your files before that." |
| 1:20 | Model Lab → Benchmarks | "Works with local models, and measures them on your hardware." |
| 1:27 | GitHub page | "Free and open source. github.com/yazhiniyanMS/nexus" |

The agent part needs a model that can produce a valid plan; a 14B+ model is more reliable than a 7B one. If a step fails on camera, keep it — and show the failure message — or re-record; never edit in a result that did not happen.

## Assets already in the repository

- `docs/assets/screenshots/*.png` — real screenshots (2000×1272) of NEXUS analysing its own repository.
- `docs/assets/demo/nexus-tour.gif` — the same screenshots as a looping tour (not a screen recording).
- `docs/assets/demo/nexus-report.html` — a real codebase report.
- `docs/assets/social-preview.png` — 1280×640 card for the GitHub social preview; source in `docs/assets/social/`.
