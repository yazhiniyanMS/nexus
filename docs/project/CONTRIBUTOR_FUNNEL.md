# Contributor funnel and feedback loop

How people find something to work on, how feedback reaches the project, and how the maintainer keeps up. Kept small on purpose: one maintainer, no external services, no tracking.

## Feedback in

Everything arrives as a GitHub issue through a form (linked from the README, the website, and **About & feedback** in the app):

| Form | Label | Purpose |
| --- | --- | --- |
| Installation problem | `installation` | Did not install or start |
| Platform report | `platform-report` | Works or not on a given system — how Windows and Linux get verified |
| Windows / Linux compatibility | `windows` / `linux` | A problem only on that system |
| Bug report | `bug` | Something broken |
| Confusing or hard to use | `ux` | Worked, but was unclear |
| Performance | `performance` | Slow or heavy |
| Feature request | `enhancement` | Something it does not do |

The labels must exist for the forms to apply them; see [../launch/GITHUB_SETUP.md](../launch/GITHUB_SETUP.md#6-labels).

## Triage (weekly, about 30 minutes)

1. Read new issues; reply to each within a week, even if only "thanks, reproduced".
2. Label severity in the title or a comment: data loss or security first, then broken core workflows, then everything else.
3. Platform reports that say "works": thank, and update the status line in README and website when a platform has several.
4. Close what will not be done, saying why (link [ROADMAP.md](../ROADMAP.md#not-planned)).
5. Turn anything self-contained and well understood into a `good first issue` with the template below.

## Good first issues

Only real gaps, each scoped so a newcomer can finish it in a few evenings. Drafts to file (not filed yet):

### 1. Index Java source files
Add Java declarations (classes, interfaces, enums, methods) and `import` resolution to `src-tauri/src/intel/extract.rs` and `resolve.rs`, with tests modelled on the existing language tests. Acceptance: a small Java project shows symbols, and the map shows its packages and imports.

### 2. Remember the map level per project
The map picks a starting depth automatically; if the user changes it, remember the choice per project (frontend only: `src/views/Intelligence.tsx`, store in `localStorage` keyed by project id, guarded with try/catch). Acceptance: switching projects and back keeps the chosen level.

### 3. Markdown export of the codebase report
`src-tauri/src/intel/report.rs` renders HTML. Add a Markdown rendering of the same content (no graph image; the module table and cycles instead) and an option in the save dialog. Acceptance: a test like `the_report_explains_the_project_without_its_code` for Markdown.

### 4. Linux hands-on check of the `.deb`
Install the latest `.deb` on Ubuntu 24.04, run through the platform-report checklist, and update [LINUX.md](../LINUX.md) with what happened. No code needed.

## Issue template for good first issues

```markdown
**What:** one sentence.
**Why it matters to users:** one sentence.
**Where:** files and functions to start from.
**Done when:** observable acceptance criteria, including the test to add.
**Help:** comment here; the maintainer replies within a week.
```

## From first contribution to regular

- Review first pull requests within a week; be specific and kind.
- Thank contributors in release notes by name (if they agree).
- Offer a second, slightly larger issue to people whose first PR went well.
