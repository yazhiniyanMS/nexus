# 30-day plan: the first 20 real users

The goal for this month is **20 people who installed NEXUS and told us how it went** — not stars. Twenty people who tried it and said what happened will shape the product more than a thousand who bookmarked it.

**Day 1 is the day 0.2.0 is published.** Before that: merge PR #2, release 0.2.0, apply the repository settings in [GITHUB_SETUP.md](GITHUB_SETUP.md) and file the good first issues. Launching earlier sends people to the 0.1.0 download, which lacks what the posts describe.

Every post is a draft the maintainer edits and posts personally ([CHANNELS.md](CHANNELS.md) has each community's rules). Skip a day rather than post filler. "Expected outcome" is a realistic hope for a one-person project with no audience yet, not a promise; record what actually happened.

**Measurement methods** used below:
- **R** — `node scripts/adoption-report.mjs --save ~/nexus-adoption` ([ADOPTION_METRICS.md](../project/ADOPTION_METRICS.md)): visitors, referrers, downloads by system, outside issues and pull requests.
- **I** — count issues by label (`platform-report`, `installation`, `ux`, `bug`) and Discussions posts.
- **P** — the post itself: comments, questions, and replies you've answered (not votes).
- **L** — your private log: three lines a day on what happened.

## Week 1 — Release and ask people you know (days 1–7)

Objective: 0.2.0 works for people other than the maintainer, on all three systems.

| Day | Deliverable | Channel | Expected outcome | Measured by |
| --- | --- | --- | --- | --- |
| 1 | 0.2.0 published; description, homepage, topics, social preview set; Discussions on with a welcome post | GitHub | Repository looks complete to a first-time visitor | R (launch baseline) |
| 2 | Install 0.2.0 from the release page on macOS, and in Windows and Linux virtual machines if you have them | — | Any first-five-minutes bug found by you, not a user | L; issues you file |
| 3 | [Launch post](LAUNCH_POST.md) | Your blog + Dev.to | A permanent page to link from everywhere else | P; R referrers (dev.to) |
| 4 | Personal message to 5 developers you know, asking them to try it on their own project for 20 minutes and send a platform report | Direct, one at a time | 2–3 agree to try | L |
| 5 | Personal message to 5 more (classmates, a club, a mentor) | Direct | 2–3 more agree | L |
| 6 | Social post 1 (launch, tour GIF) | X / Bluesky / Mastodon | A few clicks from people who follow you | R referrers |
| 7 | **Review:** answer every report; fix or document each installation problem | GitHub | Every reporter has a reply | I (target: 5 reports, ≥1 Windows, ≥1 Linux) |

## Week 2 — First public launch (days 8–14)

Objective: strangers install it and get the map or the report working without help.

| Day | Deliverable | Channel | Expected outcome | Measured by |
| --- | --- | --- | --- | --- |
| 8 | 0.2.1 if week 1 found install or first-run bugs; otherwise a TROUBLESHOOTING update | GitHub | Fewer people hit the same problem | I (`installation`) |
| 9 | **Show HN** ([draft](LAUNCH_PLAN.md#show-hn)), weekday morning US Eastern; stay 6 hours | Hacker News | Usually little traction; a good result is a handful of substantive comments and installs | P; R (visitors, news.ycombinator.com referrer, downloads) |
| 10 | Reply to every HN comment; file good criticism as issues | Hacker News, GitHub | Criticism turned into concrete issues | P; issues opened |
| 11 | **r/LocalLLaMA** ([draft](LAUNCH_PLAN.md#rlocalllama)) with your own Model Lab table (Copy table) | Reddit | Local-model users try Model Lab and say which models work | P; R referrers (reddit.com) |
| 12 | 60-second video ([script](DEMO_SCRIPT.md#60-second-version-youtube-shorts-x-the-readme)) | YouTube Shorts + X | A visual people can share | Views (as reported by the platform); R referrers |
| 13 | LinkedIn post ([draft](MORE_DRAFTS.md#linkedin)) | LinkedIn | People who know you try it or pass it on | P; L |
| 14 | **Review:** which posts brought *reports*, not just visits? Most-asked question → FAQ | GitHub | Next week's effort goes where reports came from | R + I (target: 12 people in total) |

## Week 3 — Engineering audience and contributors (days 15–21)

Objective: the first outside contribution is under way.

| Day | Deliverable | Channel | Expected outcome | Measured by |
| --- | --- | --- | --- | --- |
| 15 | [Technical article](TECHNICAL_ARTICLE.md): sandboxes, worktrees, process trees | Dev.to | Engineers who care about agent safety find the project | P; R referrers |
| 16 | **r/rust** post leading with the article's engineering | Reddit | Rust developers read the code; maybe a first PR | P; R |
| 17 | Social posts 4 and 5 (bubblewrap lesson, deadlock) | X / Bluesky / Mastodon | Credibility with developers | P |
| 18 | **Contributor call** ([draft](SOCIAL_AND_VIDEO.md#contributor-call)) pinned in Discussions and posted to **r/opensource** | GitHub, Reddit | Someone comments on a good first issue | I (comments on `good first issue`) |
| 19 | One showcase post in one Discord server (Ollama or Tauri), in the channel meant for projects | Discord | A few people from the tool's own community try it | L; R |
| 20 | Review every open pull request within 2 days; label new self-contained issues `good first issue` | GitHub | Contributors aren't left waiting | I (open PRs, age) |
| 21 | **Review:** are good first issues being picked up? If not, are they too big or unclear? | — | Issues rewritten if needed | I (target: 16 people, ≥1 outside PR) |

## Week 4 — Consolidate and plan 0.3 (days 22–30)

Objective: what people asked for becomes the next release plan.

| Day | Deliverable | Channel | Expected outcome | Measured by |
| --- | --- | --- | --- | --- |
| 22 | [Second technical post](BLOG_MAPPING_WITHOUT_AI.md): mapping a codebase without AI | Dev.to | Readers interested in code intelligence; contributors for language support | P; R |
| 23 | Video concept 2 (share a report) | YouTube Shorts + X | Shows the most distinctive feature | Platform views; R |
| 24 | Pull requests to one or two awesome lists, following their rules | GitHub | Long-term discovery (may take weeks to merge) | Merged or not |
| 25 | Update [ROADMAP.md](../ROADMAP.md) from what users reported; close what won't be done, saying why | GitHub | Users see their feedback acted on | I |
| 26 | Social post 7 (honest limits — which matters most?) | X / Bluesky / Mastodon | Replies that rank the limits | P |
| 27 | 0.2.x with the month's fixes; credit reporters who agreed to be named | GitHub Releases | Reporters see results | I (issues closed); R downloads |
| 28 | This Week in Rust entry linking the technical article | GitHub (PR) | One line read by many Rust developers | Merged or not; R |
| 29 | **"What I learned from the first month"** with real numbers from the adoption report | Blog + Dev.to | Honest record; invites the next group of testers | P; R |
| 30 | **Final review** and next month's plan | — | Decide what to keep, stop and start | R + I (target: 20 people; 3 Windows and 3 Linux reports; 2 merged outside PRs) |

## Weekly review criteria

- **Keep doing** a channel if it brought at least one report or contribution, not just visits.
- **Stop** if a community removed the post or reacted badly; learn why, and don't repost.
- **Fix before posting again** if anyone couldn't install or start NEXUS.
- **Write it down:** three lines in the log, and one public update only when there is something real to say.

## Rules

- Never post numbers you did not measure. "Benchmarks" means your machine, your prompts, stated.
- No unsolicited messages to strangers; personal asks go only to people you know.
- Show other people's code only if it is public and you link it; private code never.
- Reply to every comment within a day of posting; turn good criticism into issues.
