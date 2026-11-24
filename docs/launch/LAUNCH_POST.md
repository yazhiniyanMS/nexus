# Launch post (blog, dev.to, Hashnode, personal site)

> Draft for the maintainer to edit and publish under their own name. Bracketed parts are for you to fill in or delete — do not publish them as is. Everything else describes what NEXUS does today; keep it that way.

---

# I built a local-first workspace for understanding codebases before letting AI change them

I'm a student developer, and most of my coding time isn't spent writing new code. It's spent opening a project I didn't write — a course assignment's starter code, an open-source repo I want to contribute to, [your own example] — and trying to work out where things are and what depends on what.

AI tools made one part of that easier and another part worse. They'll happily explain a file or rewrite a function, but they start *changing* things before I understand the shape of the project, they mostly need a cloud model to explain anything, and "allow this edit?" prompts one at a time don't give me a clear picture of what I'm agreeing to.

So I built **NEXUS**: an open-source desktop app (macOS, Windows, Linux) that does two things.

## 1. Understand a codebase without AI

Point it at a folder. In a few seconds it analyses the project and builds a local code index (Rust, TypeScript, JavaScript, Python, Go), then shows you:

- an **architecture map** of modules and the imports between them, with **dependency cycles** marked;
- the **most-imported files** (change these and you touch everything);
- symbols, references and search across all your projects;
- findings like possible hard-coded secrets, committed `.env` files and missing tests.

None of that needs a model, an API key or a network connection.

And there's one feature I haven't seen elsewhere: **Share a report**. It writes a single HTML file with the graph, cycles, hotspots, dependencies and findings — and *no source code*. You can send someone the shape of a private project without sending the project. [Here's the one for NEXUS itself](https://github.com/yazhiniyanMS/nexus/blob/main/docs/assets/demo/nexus-report.html).

## 2. Let agents change it — only through you

When I do want AI help, NEXUS runs it as a proposal, not an edit:

1. A research step and an architect step read the code and propose a plan. **I approve the plan.**
2. A developer step writes the change in a **separate Git worktree** — never in my project.
3. Test commands each wait for **my approval**, and run sandboxed on macOS and Linux (the macOS sandbox or bubblewrap: no writing outside the workspace, no reading `~/.ssh` or cloud credentials, optionally no network).
4. A review step reads the diff and the test results.
5. **I approve the final diff**, which is applied all-or-nothing.

It works with local models through Ollama, llama.cpp or LM Studio, so none of this has to leave my laptop. If a file the agent edits contains a credential, the model gets a placeholder instead, and the real value is put back only when the file is written locally.

## What it isn't

It's not an editor — I still write code in [your editor]. It's not the most capable agent either; Claude Code, Cursor and OpenHands are much stronger at complex changes. NEXUS is the place I go to *understand* a project and to run AI changes I can review step by step.

It's also a beta that one person built. Windows and Linux builds pass CI install-and-launch checks but have had very little hands-on use, and the installers aren't code-signed yet.

## How it's built

A Tauri 2 app: a Rust core (SQLite, Git, pseudo-terminals, the code index, the agent engine, process sandboxing) and a React interface that can only call typed commands in the core. I wrote up the engineering — the sandbox, a deadlock I found, and how the report avoids leaking code — in [a separate article](TECHNICAL_ARTICLE.md).

## What I'd love feedback on

- Is the **codebase report** useful to you? What's missing from it?
- Does the **approve-the-plan / approve-each-command / approve-the-diff** flow feel safe, or just slow?
- If you're on **Windows or Linux**: does it install and run?

It's MIT-licensed and on GitHub: **https://github.com/yazhiniyanMS/nexus**. Issues and pull requests are very welcome.
