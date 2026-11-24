# NEXUS design

This document has two parts: the audit of the interface as it was before the design pass (November 2026), and the design system every screen now follows. When a rule here and a screen disagree, the screen is wrong.

## Part 1 — Audit

Taken from the running app (1320×840 window) and the source.

### Across the app

| Problem | Where | Effect |
| --- | --- | --- |
| No type scale: seven arbitrary sizes (10, 11, 12.5, 13, 13.5, 15, 17 px) plus Tailwind `xs`–`2xl` | Every view | Headings, labels and body text do not line up from screen to screen |
| Every page starts with the same 56 px title + subtitle header, then tabs, then a card grid | Overview, Projects, Intelligence, Git, Search, Activity, AI, Model Lab | Screens look interchangeable; the subtitle often restates the title ("Search — Find files and text…") |
| Stat cards with a big number | Overview (4), Projects (3), Agents (3) | Generic dashboard pattern; "Tokens used 19,809" and "AI providers 1" help no decision |
| Two chat surfaces (AI Workspace → Chat, Model Lab → Playground) | AI, Model Lab | Same job twice; only the Playground streams, measures and saves |
| Provider management in two places, runtime detection in two places | AI Workspace (Providers, Local models), Model Lab (Providers, Runtimes) | Users cannot tell which is authoritative |
| No Settings screen | — | Defaults, keys, usage and shortcuts are scattered or missing |
| Two confirmation styles: native OS dialog and inline two-step button | Activity, Agents, Projects, Tasks vs Model Lab | Destructive actions behave differently from screen to screen |
| Status shown with ad-hoc icon sets | Agents and Tasks each define their own `StepIcon` | Running, waiting, failed and done look different in each place |
| Monospace placeholders truncate ("runtime defau…") | Model Lab settings fields | Looks broken at the default width |
| Every primary button is a filled mint pill, including secondary actions ("New", "Import", "Search", "Run") | All screens | No hierarchy: the eye cannot find the one main action |
| Navigation is one flat list of 11 equal items | Sidebar | No grouping by job; Activity and AI Workspace sit at the same level as Projects |
| Shortcuts exist (⌘K, ⌘1–9, Esc) but are not listed anywhere in the app | — | Undiscoverable |

### Screen by screen

**Overview.** Four vanity stat cards, a project list duplicated from the Projects screen, and a global activity feed. Nothing about the selected project's state (branch, uncommitted work, findings, index freshness), nothing to resume. Verdict: redesign around the current project.

**Projects.** The project list and detail split works. Overview tab repeats the stat-card pattern (Files / Lines / Size) and spends a third of the screen on it; Files tab is a usable explorer with preview. No route to a terminal in the project. Verdict: keep structure, densify the overview, add terminal and Git entry points.

**Intelligence.** Map, Symbols, Ask, Docs are real and useful. The map sits in a large empty canvas with help text in a side column; acceptable. Verdict: align header and controls with the system; no structural change.

**Agents.** The overview is five tall role cards with struck-through permissions — decorative, repeated on every visit. The run list only shows "Overview" until a run exists. Run view (timeline, changes, logs, approvals) is functional. Verdict: replace role cards with a compact permission table, lead with runs grouped by state.

**Terminal.** Dense and purpose-built. Verdict: only token alignment.

**Model Lab.** Six tabs, functional. Providers tab duplicates provider management. Verdict: move connections and defaults to Settings; fix truncated fields.

**Search.** Clean, but the page header subtitle restates the purpose and the primary Search button competes with the field. Verdict: tighten.

**Git.** Changes, History, Branches with diffs; stage and unstage. No way to commit, so the screen cannot finish its own job. Conflicted files are listed like any other change. Verdict: add commit, a conflict section and keyboard flow.

**Tasks.** Functional list/detail. Empty state is fine. Shares the status-icon problem. Verdict: shared status component.

**AI Workspace.** Chat duplicates the Playground (without streaming, metrics or saved sessions); Providers and Local models duplicate Model Lab; Usage has no other home. Verdict: remove the screen; chat → Playground (gains "include project summary"); providers, usage → Settings.

**Activity.** A plain log with full timestamps; works. Verdict: group by day, filter by kind.

**Testing Lab.** Does not exist. It is Milestone 6 in `ARCHITECTURE.md` and is a new module, not a redesign; it is not part of this pass.

## Part 2 — Design system

### Principles

1. **One main action per view.** Only that action is filled. Everything else is secondary (bordered) or quiet (text).
2. **Density over decoration.** Lists, tables and split panes before cards. A card is used only to group controls that act together.
3. **State is information.** Colour is reserved for state (success, waiting, failure, local/remote) and focus. Nothing is coloured to decorate.
4. **Say it once.** No subtitle that restates the title; no help text that repeats a label.
5. **Every control works.** No disabled-forever buttons, no placeholders, no fake numbers. Unknown values show as a dash.

### Colour tokens (`src/index.css`)

| Token | Use |
| --- | --- |
| `bg` | Window background, inputs |
| `panel` | Sidebars, secondary panes |
| `raised` | Selected rows, hovered controls, secondary buttons |
| `hover` | Hover on raised elements |
| `line` / `line-strong` | Separators / control borders and focus-adjacent borders |
| `ink` / `dim` / `faint` | Primary text / secondary text / tertiary text and disabled |
| `accent` (mint) | The one primary action per view; success; "local" |
| `lav` (lavender) | Keyboard focus ring; selection; AI features' active tab marker |
| `ok`, `warn`, `danger` | Done, waiting/needs attention, failed/destructive |
| `info` | Running / in progress |

No gradients, no glows, no translucent glass. Backgrounds never use an accent at more than 12% opacity.

### Type scale

| Class | Size / line | Use |
| --- | --- | --- |
| `text-2xs` | 11 / 16 | Meta: timestamps, counts, badges, shortcut hints |
| `text-xs` | 12 / 16 | Secondary text, labels, table cells in dense lists |
| `text-sm` | 13 / 20 | Body text, controls, list rows — the default |
| `text-md` | 15 / 22 | View titles, section titles in detail panes |
| `text-lg` | 20 / 28 | One per screen at most: the project or object a screen is about |

Weights: 400 body, 500 labels and rows, 600 titles. Numbers that change use `tabular-nums`. Code, paths, model ids and commands use the mono stack; placeholders are never monospace.

### Spacing and size

4 px base. Gaps of 4, 8, 12, 16, 24. Pane padding 16 (20 on wide detail panes). Control heights: 24 (dense, inside rows), 28 (toolbars), 32 (forms). Toolbars are 44 px; the view header is 44 px.

### Radius and borders

`sm` 4 px — badges, keys, small controls. `md` 6 px — buttons, inputs, menus. `lg` 8 px — dialogs, popovers, the few cards. Pills (`full`) only for status dots. One-pixel borders in `line`; never two nested bordered boxes.

### Buttons

| Variant | Look | When |
| --- | --- | --- |
| `primary` | Mint fill, dark text | The single main action of a view or dialog |
| `secondary` (default) | Raised fill, `line-strong` border | Other actions |
| `ghost` | Text only, raised on hover | Toolbar and row actions |
| `danger` | Danger text and border | Destructive actions, always behind a confirmation |
| `IconButton` | 28 px square ghost, always with an accessible label and tooltip | Icon-only actions |

Busy buttons show a spinner and keep their width; disabled buttons explain why in their tooltip.

### Status

One component, `Status`, for every long-running thing (tasks, agent steps, downloads, benchmarks, servers):

| State | Mark | Colour |
| --- | --- | --- |
| idle / pending | hollow dot | faint |
| queued | hollow dot | dim |
| running | pulsing dot | info |
| waiting (needs you) | solid dot | warn |
| done | check | ok |
| failed | cross | danger |
| cancelled / skipped | dash | faint |

### Iconography

Lucide, 14 px in controls and rows, 16 px in navigation, 1.75 stroke. An icon never stands alone without a label or tooltip. No emoji.

### Confirmation

Destructive actions use the inline two-step `ConfirmButton`: the first click changes the button to a danger button that names the consequence ("Delete 3 runs", "Remove project — files stay on disk"), with Keep beside it; it reverts after six seconds. Native OS dialogs are not used for confirmation.

### Keyboard

| Keys | Action |
| --- | --- |
| ⌘K / Ctrl+K | Command palette: screens, projects, actions, symbols |
| ⌘1–⌘9 / Ctrl+1–9 | Screens in sidebar order |
| ⌘, / Ctrl+, | Settings |
| Esc | Close palette, drawer, dialog, or cancel inline confirmation |
| ↑ ↓ Enter | Move and open in lists and the palette |
| ⌘Enter / Ctrl+Enter | Submit the multi-line field in focus (commit message, prompt, task description) |

Every interactive element is reachable with Tab, shows the lavender focus ring, and has an accessible name.

### Motion

Only to show a change of state: 120 ms colour transitions on hover/press, a pulse on running status, 150 ms slide for drawers. Nothing animates on load. `prefers-reduced-motion` turns all of it off.

### Layout patterns

Each screen uses the pattern that fits its job rather than a shared template:

- **Workspace (Overview):** document layout about one project — identity, repository state, what needs attention, recent work.
- **List + detail:** Projects, Tasks, Agents, Git history.
- **Three-pane work surface:** Git changes (files · diff · commit), Playground (sessions · conversation · settings).
- **Canvas:** Intelligence map, Terminal.
- **Settings:** section list + form.

## Part 3 — Status of the migration

Done:

- Every screen uses the shared header (`ViewHeader`, 44 px), the type scale (no arbitrary font sizes remain), `Status` for run and step states, `ConfirmButton` for destructive actions, and `Tabs` (ARIA tabs: only the selected tab is in the Tab order; ←/→, Home and End move between them).
- AI Workspace is removed: chat → Playground (with "use a summary of the project"), connections, defaults and token usage → Settings. Model Lab's Providers tab moved to Settings.
- Navigation is grouped (Workspace, Automation, Models, then Activity); ⌘1–9 follow that order; ⌘, opens Settings.
- Overview, Git, Agents, Projects and Activity were rebuilt to the patterns above. Git commits what is staged and shows merge and rebase state and conflicts.
- Checked at 1320×840 and at the 980×640 minimum: no screen scrolls sideways and no header clips.

Not yet:

- Model Lab's Runtimes, Benchmarks and Compare tabs still use bordered `Card` sections; they are consistent with the tokens but heavier than the rest.
- Code-split chunk for the file viewer (Monaco) is large (≈3.3 MB); it is loaded only when a file is opened.
