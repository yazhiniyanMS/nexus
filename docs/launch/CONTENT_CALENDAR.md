# 30-day plan: the first 20 real users

The goal for this month is **20 people who installed NEXUS and told us how it went** — not stars. Twenty people who tried it and said what happened will shape the product more than a thousand who bookmarked it. Reach comes later, from people who found it useful.

**Day 1 is the day 0.2.0 is published.** Before that: merge PR #2, release 0.2.0, apply the repository settings in [GITHUB_SETUP.md](GITHUB_SETUP.md) and file the good first issues. Launching earlier sends people to the 0.1.0 download, which lacks what the posts describe.

Every post is a draft the maintainer edits and posts personally ([CHANNELS.md](CHANNELS.md) has the rules for each place). Skip a day rather than post filler. Measure with `node scripts/adoption-report.mjs --save ~/nexus-adoption` ([ADOPTION_METRICS.md](../project/ADOPTION_METRICS.md)).

## Week 1 — Release and ask people you can reach directly (days 1–7)

**Objective:** 0.2.0 works for people other than the maintainer, on all three systems.

| Day | Deliverable / activity |
| --- | --- |
| 1 | 0.2.0 published. Repository description, homepage, topics and social preview set. Discussions on, with a pinned welcome post. Run the adoption report for the launch baseline |
| 1 | Install 0.2.0 from the release page yourself on macOS, and in a Windows and a Linux virtual machine if you have them; fix anything in the first five minutes |
| 2 | [Launch post](LAUNCH_POST.md) on your blog and dev.to |
| 2–4 | **Personal asks:** message 10 developers you actually know (classmates, a club, a mentor) and ask each to try it on their own project for 20 minutes and send a platform report. One personal message each — no group blasts |
| 3 | Social post 1 (launch, with the tour GIF) |
| 5 | Social post 9 (asking for Windows/Linux testers) |
| 6–7 | Answer every report; fix or document each installation problem |

**Feedback target:** 5 platform reports, including at least 1 Windows and 1 Linux.
**Review (day 7):** Did anyone fail to install or launch? If yes, fixing that comes before any public post in week 2.

## Week 2 — First public launch (days 8–14)

**Objective:** strangers can install it and get the map or the report working without help.

| Day | Deliverable / activity |
| --- | --- |
| 8 | Ship 0.2.1 if week 1 found installation or first-run bugs |
| 9 | **Show HN** ([draft](LAUNCH_PLAN.md#show-hn)), a weekday morning US Eastern time; stay for 6 hours and reply to everything |
| 10 | Social post 2 (the report) |
| 11 | **r/LocalLLaMA** ([draft](LAUNCH_PLAN.md#rlocalllama)) with your own Model Lab numbers |
| 12 | LinkedIn post ([draft](MORE_DRAFTS.md#linkedin)) |
| 13 | Record video concept 1 ("map a repo in 30 seconds") from [SOCIAL_AND_VIDEO.md](SOCIAL_AND_VIDEO.md) |
| 14 | Weekly review; turn the most-repeated question into an FAQ entry |

**Feedback target:** 12 people in total who told us how it went; every issue answered within 2 days.
**Review (day 14):** Which post brought visitors (referrers) and which brought *reports*? Do more of the second kind.

## Week 3 — Engineering audience and contributors (days 15–21)

**Objective:** the first outside contribution is under way.

| Day | Deliverable / activity |
| --- | --- |
| 15 | [Technical article](TECHNICAL_ARTICLE.md) on dev.to |
| 16 | **r/rust**, leading with the engineering and linking the article |
| 17 | Social posts 4 and 5 (sandbox lesson, deadlock) |
| 18 | **Contributor call** ([draft](SOCIAL_AND_VIDEO.md#contributor-call)) as a pinned Discussion, and on **r/opensource** |
| 19 | One Discord showcase post (Ollama or Tauri server) |
| 20 | Review and reply to any pull request within 2 days; label new self-contained issues `good first issue` |
| 21 | Weekly review |

**Feedback target:** 16 people; at least 1 pull request from someone else.
**Review (day 21):** Are good first issues being picked up? If not, are they too big or badly described?

## Week 4 — Consolidate and plan 0.3 (days 22–30)

**Objective:** what people asked for becomes the next release plan, and new people can still find their way in.

| Day | Deliverable / activity |
| --- | --- |
| 22 | Record video concept 2 (share a report) or 3 (an agent that has to ask) |
| 23 | Social post 3 or 6 |
| 24 | Pull requests to one or two awesome lists and This Week in Rust, following their rules |
| 25 | Update [ROADMAP.md](../ROADMAP.md) from what users reported; close what won't be done, saying why |
| 26 | Social post 7 (honest limits — ask which one matters most) |
| 27 | Release 0.2.x with the fixes from the month, crediting reporters who agreed to be named |
| 28 | **"What I learned from the first month"** post with real numbers from the adoption report |
| 29–30 | Final review; write next month's plan |

**Feedback target:** 20 people who told us how it went; 3 platform reports each from Windows and Linux; 2 merged outside contributions.

## Metrics to record each week

| Measure | Why |
| --- | --- |
| People who reported how it went (reports, issues, Discussions, replies) | The real goal |
| Installation problems opened / fixed or documented | Whether first use works |
| Platform reports by system | Whether Windows and Linux really work |
| Unique repository visitors and top referrers | Which channels bring people |
| 0.2.x installer downloads by system | Interest by platform (not users) |
| Outside pull requests and contributors | Whether a community is forming |
| Stars | Recorded, not a goal |

## Weekly review criteria

- **Keep doing** a channel if it brought at least one report or contribution, not just visits.
- **Stop** if a community removed the post or reacted badly; learn why, and don't repost.
- **Fix before posting again** if anyone couldn't install or start NEXUS.
- **Write it down:** three lines in a private log, and one public update only when there is something real to say.

## Rules

- Never post numbers you did not measure. "Benchmarks" means your machine, your prompts, stated.
- Show other people's code only if it is public and you link it; show private code never.
- Reply to every comment within a day of posting; turn good criticism into issues.
