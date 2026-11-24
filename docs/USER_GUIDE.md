# User guide

The sidebar groups screens by job: **Workspace** (Overview, Projects, Search, Git, Terminal), **Automation** (Tasks, Agents), **Models** (Intelligence, Model Lab), then Activity. The project chosen at the top of the sidebar applies to every screen.

## Overview

One project at a glance: name, path, languages and frameworks, file and line counts, when it was analysed and indexed. Below that:

- **Repository** — branch, upstream, ahead/behind, conflicts and up to eight changed files.
- **Agent tasks that need you** — shown only when there are some.
- **Findings** — by severity, each linked to the file and line.
- **Recent in this project** — the activity for this project only.

The action row opens Files, Git, Terminal, Ask, a new agent task, and **Share a report**.

## Codebase report

**Overview → Share a report** (or **Intelligence → Docs → Report (HTML)…**) saves one self-contained HTML file and opens it in your browser. It explains the project to someone who has never seen it:

- totals (files, lines, symbols, modules, dependency cycles, declared dependencies, high-severity findings);
- languages;
- an architecture graph of modules and the imports between them, with cycles marked;
- where to start reading (conventional entry points) and the most-imported files;
- external packages in use and declared dependencies per manifest;
- findings, and Git facts (branch, number of commits and contributors, first and last commit dates).

**It contains no source code.** It lists file paths, module and package names and counts; findings that would quote a line of code show only their location. It has no scripts and loads nothing. If the project has no code index yet, it is built first. Sample: [nexus-report.html](assets/demo/nexus-report.html).

## Projects

Import folders (they are never moved or changed). Tabs: **Files** (a read-only viewer), **About** (summary, languages, top folders), **Dependencies** and **Findings**. The header opens the project in Terminal, Git or Intelligence, re-analyses it, or removes it from NEXUS (files untouched; removal waits until no agent task is running in it).

## Intelligence

Built from a code index of Rust, TypeScript, JavaScript, Python and Go (**Rebuild index** after large changes; there is no file watcher).

- **Map** — modules at one, two or three folder levels, or the file graph; optional external packages. Click a node for what it imports and what imports it; drag to move, scroll to zoom. Red outlines mark dependency cycles.
- **Symbols** — ranked symbol search with references and a file preview.
- **Ask** — a question about the code, answered by a model with the files and lines it used. Works with a local model; a remote one asks you to confirm first. Credentials in the excerpts are masked before they are sent.
- **Docs** — a deterministic architecture document (Markdown), optionally with a model-written narrative; and the HTML report.

## Search

File names and contents across every project, as text or a regular expression, optionally case-sensitive. Respects `.gitignore` and skips generated folders such as `node_modules` and `target`.

## Git

- **Changes** — conflicts, staged, changed and untracked files; ↑/↓ to move through them; the diff on the right. Stage or unstage one file or a whole group; **Mark all resolved** after a merge. Write a message and commit what is staged with **⌘Enter** / **Ctrl+Enter**. Committing is refused with nothing staged or with unresolved conflicts.
- **History** — commits with their patches.
- **Branches** — local and remote branches. NEXUS never switches branches, pulls or pushes.

Git runs with your own configuration, except that a repository's file-system monitor, external diff tools and text-conversion filters are switched off when NEXUS only reads (it reads automatically, so those settings must not run programs on their own).

## Terminal

Your own shell (zsh, bash or fish; PowerShell or Command Prompt on Windows) in real pseudo-terminals, starting in the project folder. Tabs, split panes you can resize, restart, and search across saved output. Output is saved per terminal (about 1–2 MB) and replayed after a restart; shells themselves end when NEXUS closes. Only your typing and pasting reach a terminal — no task, agent or model can write to one.

## Tasks

A task is a list of steps; a step is something you do by hand or a command. Add steps yourself, let NEXUS **suggest** a verification workflow from the project's manifests, or **plan with AI**.

**Nothing runs without approval.** Each command waits for **Approve & run** and shows how it will run (level, sandbox, time limit). Destructive-looking commands (deleting, force-pushing, `sudo`, piping a download into a shell…) are flagged. Output streams live; **Cancel** stops the command and everything it started. A task can set its own time limit per step.

## Agents

Describe an outcome. Five roles work through a fixed pipeline:

1. **Research** reads the relevant code (read-only).
2. **Architect** analyses the structure and proposes a plan (read-only). **You approve the plan.**
3. **Developer** writes the change in a separate Git worktree, never in your project. Credentials in the files it edits are sent to the model as placeholders and restored only in the files written.
4. **Testing** finds test commands in the project or the plan. **You approve each command**; it runs in the worktree at the agent execution level.
5. **Review** reads the diff and the test results and gives a verdict.
6. **You approve the final diff.** It is applied to your working tree all-or-nothing — not staged, not committed — and refused if it clashes with edits you made meanwhile.

The permission table on the Agents screen lists exactly what each role may do; the core enforces it. Runs can be paused, resumed (also after a restart) and cancelled; the full trace is kept. Agents need a Git repository with at least one commit and a model connection.

## Model Lab

Local runtimes (detect, start and stop Ollama and llama.cpp servers bound to this computer), a model library with downloads you start yourself, a streaming playground, repeatable benchmarks on your hardware, and side-by-side comparison. See [MODEL_LAB.md](MODEL_LAB.md).

## Activity

A local log of imports, analyses, commands run, commits, task and agent results and AI usage, grouped by day and filterable by kind and project.

## Settings

Model connections, model defaults, **Execution** (levels, time limits, limits, allowed environment variables), token usage, keyboard shortcuts and general options. See [CONFIGURATION.md](CONFIGURATION.md).

## Keyboard

| Keys (macOS / Windows and Linux) | Does |
| --- | --- |
| ⌘K / Ctrl+K (also Ctrl+Shift+P on Windows and Linux) | Command palette: screens, projects, actions and code symbols |
| ⌘1…⌘9 / Ctrl+1…9 | Overview, Projects, Search, Git, Terminal, Tasks, Agents, Intelligence, Model Lab |
| ⌘, / Ctrl+, | Settings |
| ⌘Enter / Ctrl+Enter | Commit (in the commit message) |
| ↑ ↓ Enter | Move through and open items in lists and the palette |
| ← → Home End | Move between tabs |
| Esc | Close the palette, the file drawer or a dialog; cancel a pending confirmation |

On Windows and Linux, Ctrl combinations typed inside a terminal go to the shell; copy and paste there are Ctrl+Shift+C / Ctrl+Shift+V.
