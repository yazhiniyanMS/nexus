# Positioning

November 2026. Based on what NEXUS actually does today and on the competitors' public documentation ([COMPARISON.md](../COMPARISON.md)).

## One line

**Understand a codebase on your own machine — then change it only through you.**

## Ten seconds

NEXUS is a free, open-source desktop app that maps any local codebase without AI — modules, dependency cycles, hotspots, a shareable report — and lets AI agents propose changes in an isolated copy that reach your files only after you approve the diff. It works with local models and sends nothing anywhere unless you say so.

## Short description (GitHub "About", 350 characters max)

> Local-first desktop workspace to understand any codebase without AI — architecture map, cycles, hotspots, shareable report — and let AI agents change it only through approvals, in an isolated Git worktree with sandboxed commands. Works with Ollama and other local models. macOS, Windows, Linux. MIT.

## Who it is for first

**Developers who regularly land in code they did not write and do not want to send it to a cloud service to understand it.**

Concretely:

1. **Students and new contributors** opening an unfamiliar open-source or course project.
2. **Developers joining a team or inheriting a repository** who need to know where things are and what depends on what.
3. **Privacy-conscious developers trying local models** (Ollama, llama.cpp) who want agents but not unsupervised ones.

Not first: people who want an AI editor with completions (Cursor, Windsurf), or the most capable autonomous agent (Claude Code, OpenHands).

## The problem

AI tools made changing code fast and understanding code optional. Agent-first tools start editing before you know what the code looks like, need a cloud model to explain anything, and apply changes to your files with a prompt-by-prompt "allow". Teams cannot easily share the *shape* of a private codebase without sharing the code.

## What NEXUS does better (and only what is true)

| Differentiator | Why it is real |
| --- | --- |
| **Understanding without AI** | The map, cycles, hotspots, symbols, references, search and the architecture document come from a local code index. No model, no key, no network. |
| **A report you can share without the code** | One HTML file: graph, hotspots, dependencies, findings, Git facts. Tested to contain no source text. No competitor in [COMPARISON.md](../COMPARISON.md) offers this. |
| **Changes as gated proposals** | The agent works in a separate Git worktree; plan, each command and the final diff need approval; the diff applies all-or-nothing and refuses to clash with your edits. |
| **Honest sandboxing** | Three levels described as enforced on your machine; refused, not silently downgraded, when unavailable. (Claude Code also sandboxes with Seatbelt/bubblewrap, so this is not unique — the *refuse instead of downgrade* default and the per-command record are.) |
| **Local models, measured** | Model Lab benchmarks models on your hardware before you choose one. |

Not differentiators (do not claim them): "local-first AI desktop app" (OpenCode desktop, CLODEx, PI-Desktop and others exist), "sandboxed agents" on its own, "privacy-first" on its own.

## What to show

1. Add a repository → map appears in seconds, with cycles in red.
2. **Share a report** → open the HTML file in a browser.
3. Git: review a diff and commit with ⌘Enter.
4. A task step waiting for approval that says "Runs Restricted (macOS sandbox) · time limit 30 min".
5. An agent proposing a change and stopping at the plan gate (needs a model).

## What holds adoption back today

| Blocker | Severity | Status |
| --- | --- | --- |
| Released 0.1.0 lacks the report, guided setup, sandboxing and the new interface | High | In `main`; needs CI on Windows/Linux and a 0.2.0 release |
| Unsigned installers (Gatekeeper and SmartScreen warnings) | High | Needs certificates (paid); documented workaround |
| No website live (GitHub Pages not enabled) | Medium | Site ready in `website/`; one setting to enable |
| Repository has no topics, homepage or social preview | Medium | Prepared; needs the owner to apply |
| Agents need a capable model; small local models often fail validation | Medium | Documented; roadmap item |
| Windows has no sandbox; Windows/Linux little hands-on use | Medium | Documented; roadmap item |
| Not an editor | By design | Positioned as a companion |

## Messaging rules

- Lead with understanding, then control. Never lead with "AI".
- Every claim must be demonstrable in the app or tested. Say "beta" and "one developer".
- Name competitors only with facts and links; say where they are stronger.
- No user counts, testimonials or benchmarks we have not run.
