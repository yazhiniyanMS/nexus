# Where to talk about NEXUS, and how

For each channel: why NEXUS belongs there, the format that works, the rules that matter, and when. Drafts live in [LAUNCH_PLAN.md](LAUNCH_PLAN.md), [MORE_DRAFTS.md](MORE_DRAFTS.md) and [SOCIAL_AND_VIDEO.md](SOCIAL_AND_VIDEO.md). **Nothing is posted automatically; the maintainer posts each one personally.**

The rule summaries below are from each community's public guidelines and common practice; they change, so **read the current rules (sidebar, pinned posts, `/rules`) the day you post**. When a community says no self-promotion, don't post there.

## Prerequisites for every channel

- 0.2.0 released, so the download matches what the post describes.
- The website live (done) and set as the repository homepage.
- You can reply to comments for the next 24 hours.
- One link per post: the repository, or the website if the channel prefers a homepage.

## Channels

| Channel | Why NEXUS belongs there | Format | Rules that matter | When |
| --- | --- | --- | --- | --- |
| **Show HN** | Developers who try new tools the day they see them; something installable and free is exactly what Show HN is for | `Show HN:` title, the repo link, a first comment in your own words: what it is, why you built it, honest limits | It must be something people can try now. No asking anyone to upvote, no vote rings, no reposting the same Show HN soon after. Blog posts aren't Show HN. [Guidelines](https://news.ycombinator.com/showhn.html) | Week 2, a weekday morning US Eastern time; stay for the first 6 hours |
| **r/LocalLLaMA** | Local-model users are the most natural first audience: Model Lab and local agents with approvals | Text post with your **own measured** numbers and a screenshot; ask which models work for others | Self-promotion is tolerated only from people who take part in the community; read the sidebar rules on project posts. Answer technical questions in detail | Week 2 |
| **r/rust** | The core is Rust; sandboxing, process trees and a line-based indexer are interesting engineering | Lead with the engineering (link the technical article), the project second; or the weekly "What's everyone working on" thread | Posts must be about Rust; pure product announcements get removed | Week 3 |
| **r/opensource** | An MIT project looking for testers and contributors | Short post: what it is, what help is wanted (Windows/Linux reports, Java indexing) | Projects must be open source; be explicit about the licence | Week 3 |
| **r/commandline, r/programming** | Not a fit: NEXUS is a desktop app, and r/programming removes most project announcements | — | — | Skip |
| **Dev.to** | Long-form posts get found through search for months; tags reach the right readers | The [launch post](LAUNCH_POST.md) week 1, the [technical article](TECHNICAL_ARTICLE.md) a week later. Tags: `opensource`, `rust`, `tooling`, `ai` (four maximum) | Disclose that it's your project; set `canonical_url` if it's first on your own blog | Weeks 1 and 3 |
| **GitHub Discussions** | Where users ask questions that aren't bugs; a public FAQ that grows by itself | Categories Q&A, Ideas, Show and tell, Announcements ([setup](GITHUB_SETUP.md#5-discussions-optional)); one announcement per release | Only turn it on if you'll answer weekly; an empty or unanswered forum hurts | Turn on in week 1 |
| **Discord communities** (Ollama, Tauri, Rust community servers) | Where the tools NEXUS builds on are discussed | One post in the server's showcase or self-promotion channel; afterwards answer questions in help channels without linking NEXUS unless it actually answers the question | Post only in channels meant for projects; never DM people | Weeks 2–4, one server per week |
| **X / Bluesky / Mastodon** | Developers follow builders; short videos travel | One post with a GIF or 30-second clip; then a thread of real engineering moments ([drafts](SOCIAL_AND_VIDEO.md#social-posts)) | No automated posting, no hashtag stuffing (one or two), no tagging people who didn't ask | 2–3 posts a week from week 1 |
| **YouTube Shorts** | Developers search YouTube for "visualize codebase" and "local LLM" — a short shows the map faster than any text | Vertical, under 60 seconds, captions burned in, no music needed: the [60-second script](DEMO_SCRIPT.md#60-second-version-youtube-shorts-x-the-readme) and video concepts 1, 2 and 4 in [SOCIAL_AND_VIDEO.md](SOCIAL_AND_VIDEO.md). Title says what it shows ("Mapping a codebase without AI"), link in the description | Real screen recordings only; say "sped up" if it is. Use YouTube's altered-content disclosure if anything is synthetic (nothing should be) | One short a week from week 2 |
| **LinkedIn** | Classmates, mentors and recruiters; good for credibility, rarely for users | One personal post ([draft](MORE_DRAFTS.md#linkedin)) with the tour GIF | Personal voice; no engagement-bait ("comment YES for the link") | Week 2 |
| **Product Hunt** | Broad launch audience | [Draft](MORE_DRAFTS.md#product-hunt) | Don't ask for upvotes; launches are judged on day one. Unsigned-app warnings make a poor first impression there | Only after installers are signed — not in this 30 days |
| **Awesome lists** (awesome-tauri, awesome-rust, local-LLM tool lists) | Long-term discovery | A pull request adding one line, following each list's format and inclusion rules | Many require a minimum age or stars; check before opening the PR | Week 4 |
| **This Week in Rust** | One line read by many Rust developers | Pull request to the "Project/Tooling Updates" section linking the technical article | Must link a post or release, not just the repository | Week 4 |

## What not to do, anywhere

- No mass DMs, no cold emails to strangers, no posting the same text in several places on the same day.
- No asking friends to star, upvote or comment. No bots, no bought stars or followers.
- No numbers you didn't measure, no claims the 0.2.0 download doesn't support.
- If a post is removed, read why, accept it, and don't repost elsewhere in that community.
