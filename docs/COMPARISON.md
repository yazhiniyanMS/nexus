# How NEXUS compares

Written November 2026 from each product's public documentation. Products change quickly; corrections are welcome as issues or pull requests.

## The short version

NEXUS is a **desktop workspace for understanding a codebase and controlling AI changes to it**. It is not an editor and not the strongest coding agent. Its bet is that before an AI changes your code, you should be able to see the code's shape yourself — without AI — and that every change should pass through you.

| If you want… | Probably use |
| --- | --- |
| The most capable agent working in your terminal | Claude Code |
| An AI-first editor with completions and inline edits | Cursor, Windsurf, VS Code with Copilot, Zed |
| A terminal pair programmer that commits each change, with any model | Aider |
| An autonomous agent that turns issues into pull requests | OpenHands |
| To understand an unfamiliar codebase locally (map, cycles, hotspots, a shareable report), and to run AI changes as reviewable, approval-gated proposals with local models | **NEXUS** |

They combine well: many NEXUS users will keep an editor and another agent, and use NEXUS to understand a project and review what changed.

## Side by side

| | NEXUS | Claude Code | Cursor / Windsurf | Zed | Aider | Cline | OpenHands |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Form | Desktop app | Terminal (also IDE and desktop integrations) | AI editor | Editor | Terminal | VS Code extension | Web app / CLI, runs in containers |
| Open source | Yes (MIT) | No | No | Yes | Yes | Yes | Yes |
| Code editor | No (read-only viewer) | No (uses yours) | Yes | Yes | No | Yes (VS Code) | No |
| Codebase map without AI (modules, cycles, hotspots) | Yes | No | No | No | Repo map for the model, not shown as a map | No | No |
| Shareable report without source code | Yes | No | No | No | No | No | No |
| Local models | Yes (Ollama, llama.cpp, OpenAI-compatible) | Not the default; via compatible gateways | Limited | Yes | Yes | Yes | Yes |
| Change review before it touches your files | Agent works in a separate Git worktree; you approve plan, each command and the final diff | Permission prompts per action; edits apply to your files | Diffs to accept in the editor | Diffs in the editor | Applies and commits each change (easy to undo with Git) | Approval per action | Works in its own sandbox; produces a branch or PR |
| Command sandboxing | macOS sandbox / bubblewrap, three levels; refused (not downgraded) when unavailable; none on Windows | Seatbelt / bubblewrap with a network proxy; by default falls back to unsandboxed with a warning | — | — | — | — | Docker containers |
| Benchmark local models on your hardware | Yes (Model Lab) | No | No | No | No | No | No |
| Maturity | Beta, one developer | Mature, large team | Mature | Mature | Mature | Mature | Mature |

"—" means the product does not offer this as a feature, as far as its documentation shows.

## Where NEXUS is weaker

- **Agent capability.** The pipeline is careful, not clever: it plans, writes whole files and stops at gates. Claude Code, Cursor and OpenHands are far more capable at complex changes, especially with frontier models.
- **No editor, no completions.** You still need an editor.
- **Platforms.** Daily use is on macOS. Windows has no sandbox backend yet; Windows and Linux builds have had little hands-on use.
- **Scale and support.** One student developer, beta software. Expect rough edges.

## Sources

- Claude Code sandboxing: [code.claude.com/docs/en/sandboxing](https://code.claude.com/docs/en/sandboxing), [Anthropic engineering: Claude Code sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing)
- Aider: [aider.chat](https://aider.chat)
- Cline: [cline.bot](https://cline.bot)
- OpenHands: [github.com/All-Hands-AI/OpenHands](https://github.com/All-Hands-AI/OpenHands)
- Zed: [zed.dev](https://zed.dev)
- Cursor: [cursor.com](https://cursor.com); Windsurf: [windsurf.com](https://windsurf.com)
