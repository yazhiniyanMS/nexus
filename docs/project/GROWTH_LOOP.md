# Sharing as a growth loop

People find tools through what other people share. NEXUS's sharing is designed so that sharing is always a deliberate action by the user, shares only what they can see before sharing, and never leaks code or private details. NEXUS itself sends nothing anywhere.

## Principles

1. **Opt-in, every time.** Nothing is shared automatically. Each share is a button the user presses, producing text or a file they can read first.
2. **Shapes, not code.** Names, counts and measurements — never source text. Each format has a test that checks what it leaves out.
3. **No tracking in shared artifacts.** No pixels, no tracked links, no IDs. The only addition is a plain link to the repository, so a reader can find where it came from.
4. **Say what it is.** Each share says how it was made and what it doesn't cover (a benchmark is speed on one machine, not quality).

## Built (in 0.2.0)

| What | Where | What it contains | What it never contains | Verified by |
| --- | --- | --- | --- | --- |
| **Codebase report** (HTML) | Overview → Share a report | Module graph, hotspots, dependencies, findings, Git facts; a footer link to the repository | Source code; it loads nothing (`default-src 'none'`) | `the_report_explains_the_project_without_its_code` (Rust) |
| **Architecture map as Mermaid** | Intelligence → Map → Copy as Mermaid | Module or file names and imports, cycles highlighted; a credit comment | Source code | `tests/share.test.mjs`; rendered with Mermaid 11 (16 nodes, 29 edges for NEXUS itself) |
| **Benchmark table** (Markdown) | Model Lab → Compare → Copy table / Save… | Model names, local/remote, machine (system, CPU, cores, memory), settings, date, measurements | The benchmark's name, prompts, outputs, server addresses | `tests/share.test.mjs`; checked in the app that a planted benchmark name, prompt and address were absent |
| **Architecture document** (Markdown) | Intelligence → Docs → Copy / Save (already in 0.1.0) | Modules, dependencies and cycles from the index; optionally an overview written by a model you choose | Nothing is guaranteed if a model wrote part of it — review before sharing | existing |

## Where shared things can go (the loop)

- A README section "Architecture" with the Mermaid map — every visitor to that repository sees it, with the credit comment in the source.
- An issue or pull request explaining a refactor: the map before and after.
- A report sent to a new teammate, a reviewer or a mentor before they have access to the code.
- r/LocalLLaMA or a blog post with a benchmark table — honest numbers with their conditions.
- **Show and tell** in GitHub Discussions, once enabled: "what the map showed me about my project".

## Proposed, not built

- **A "Made with NEXUS" README badge** that links to the repository — only as a snippet users can copy if they want, never added automatically.
- **A share image of the map** (PNG) for social posts. Needs care to keep labels readable and to make clear it shows names only.
- **A Markdown version of the report** (also a good first issue).
- **Comparing two reports** (before/after a refactor) — useful, and a natural thing to share.

None of these will add telemetry, accounts or uploads.
