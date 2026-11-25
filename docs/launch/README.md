# Launch kit

Prepared, not published. Nothing in this folder has been posted anywhere.

| File | What it is |
| --- | --- |
| [LAUNCH_PLAN.md](LAUNCH_PLAN.md) | Blockers to clear first, order of channels, platform-specific drafts, good-first-issue candidates |
| [LAUNCH_POST.md](LAUNCH_POST.md) | The announcement, in your voice, to edit and publish |
| [BLOG_MAPPING_WITHOUT_AI.md](BLOG_MAPPING_WITHOUT_AI.md) | Second technical post: how the map is built without AI, and where line-based indexing is wrong |
| [TECHNICAL_ARTICLE.md](TECHNICAL_ARTICLE.md) | An engineering write-up: IPC boundary, worktrees, sandboxing, process trees, a deadlock, credential placeholders, the report |
| [DEMO_SCRIPT.md](DEMO_SCRIPT.md) | 90- and 60-second video scripts, and the assets already captured |
| [LAUNCH_DAY.md](LAUNCH_DAY.md) | **Launch day 0.2.0**: final Show HN, Reddit, X posts with live links, and the order for the day |
| [producthunt/LISTING.md](producthunt/LISTING.md) | Product Hunt fields ready to paste, gallery images (1270×760) and thumbnail, launch-date recommendation |
| [CHANNELS.md](CHANNELS.md) | Each channel: why NEXUS belongs there, format, rules, timing |
| [MORE_DRAFTS.md](MORE_DRAFTS.md) | Product Hunt, LinkedIn, a short introduction, Dev.to |
| [SOCIAL_AND_VIDEO.md](SOCIAL_AND_VIDEO.md) | Five demo-video concepts, ten social posts, a contributor call |
| [CONTENT_CALENDAR.md](CONTENT_CALENDAR.md) | The 30-day plan: weekly goals, deliverables, outreach, metrics, review |
| [GITHUB_SETUP.md](GITHUB_SETUP.md) | Repository settings to apply (description, topics, Pages, social preview, security reporting) |
| [../project/POSITIONING.md](../project/POSITIONING.md) | Positioning, audience, differentiators, adoption blockers |

## Regenerating the visuals

Screenshots were taken from the real app on a throwaway data folder (`NEXUS_DATA_DIR`) with a copy of this repository at a neutral path, so no personal names or paths appear. The sample report comes from the same code the app uses:

```bash
NEXUS_REPORT_FOR=/path/to/project NEXUS_REPORT_OUT=docs/assets/demo/nexus-report.html \
  cargo test --manifest-path src-tauri/Cargo.toml --lib write_report_for_a_folder -- --ignored
```

The social preview is `docs/assets/social/social-preview.html` rendered at 1280×640 in any browser.
