# Measuring adoption without tracking anyone

NEXUS has no telemetry, analytics or update checks, and that stays true ([PRIVACY.md](../PRIVACY.md)). Adoption is measured only from data GitHub already shows the repository owner, and from what people choose to tell us.

Run it weekly (same day each week):

```bash
node scripts/adoption-report.mjs --save ~/nexus-adoption
```

It prints a short table and saves a dated snapshot in a private folder outside the repository, then compares with the previous snapshot. GitHub keeps traffic for only 14 days, so the snapshots are the history. Don't commit the snapshots; traffic numbers are the owner's, not public.

## What is measured, and what it really means

| Question | Measure | Source | What it does *not* tell you |
| --- | --- | --- | --- |
| Are people finding it? | Unique repository visitors (14 days), referrers | GitHub traffic API | Website visitors: GitHub Pages has no statistics, and we add none. Visitors who arrive at the repository *from* the website show up as the referrer `yazhiniyanms.github.io` |
| Are they trying it? | Installer downloads per system, per release | Release assets | Users. A download is a click on a file — including retries and people who never open it. Checksums and the manifest are excluded |
| Does it install and start? | Platform reports; installation-problem issues | Issue forms and labels | Silent failures. Ask for reports in every launch post and in the app's **About & feedback** |
| Do people get value the first time? | Platform reports with "the architecture map appeared" or "saved a codebase report" ticked; "Show and tell" Discussions | Issue form checklist; Discussions | Anyone who succeeded and said nothing (most people) |
| Do they come back? | Issues and comments from the same people over weeks; repeat contributors | Issue history | Returning users in general. **Not measurable without telemetry, and we won't add telemetry.** Say so if asked |
| Is a community forming? | Issues and pull requests from people other than the maintainer; contributors | Issues, pulls, contributors | Quality. Read them |
| General interest | Stars, forks | Repository | Use. Stars are bookmarks |

Never report downloads or stars as users. In posts, say "N downloads of 0.2.0" or "N people sent platform reports", never "N users".

## Baseline — 24 November 2026 (before any launch)

From `scripts/adoption-report.mjs`:

| Measure | Value |
| --- | --- |
| Stars / forks | 4 / 0 |
| Unique repository visitors, last 14 days | 7 (top referrer: Instagram, 4 unique) |
| Unique clones, last 14 days | 36 (mostly this repository's own CI) |
| 0.1.0 installer downloads: macOS / Windows / Linux | 2 / 4 / 3 |
| Issues or pull requests from others | 0 |
| Contributors other than the maintainer | 0 |

## Targets for the first 30 days after 0.2.0

Small and about real use, not reach:

- **20 people who installed it and told us how it went** (platform reports, issues, Discussions or direct replies) — the main goal.
- At least **3 platform reports each from Windows and Linux**.
- **Every installation problem answered within 2 days**, and fixed or documented within the month.
- **2 contributors** other than the maintainer with a merged pull request.
- Stars and downloads are recorded but are not targets.

## Weekly review (15 minutes, after the report)

1. Which referrer or post brought visitors this week? Do more of what worked; drop what didn't.
2. What did people who tried it run into? Each new installation problem becomes a fix or a TROUBLESHOOTING entry.
3. Is anyone waiting for a reply? Answer before anything else.
4. Write three lines in a private log: what changed, what we learned, what we'll do next week.
