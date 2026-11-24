# Technical article

> Draft for the maintainer to publish. Every detail is from the NEXUS codebase and its audit records ([docs/audit/](../audit/)); file references are real.

---

# Gating AI changes on a laptop: sandboxes, worktrees and a report that leaks no code

NEXUS is a Tauri 2 desktop app with a Rust core and a React interface. It indexes a codebase locally, draws its architecture, and runs AI agents whose every change needs approval. This post is about the parts that were harder than they looked.

## The trust boundary is the IPC layer

A Tauri webview can only call commands the Rust core registers. NEXUS grants the webview no file-system, shell or opener permissions at all; 126 typed commands are the whole surface, and the production Content-Security-Policy forbids remote and inline scripts. That matters because one of those commands writes to your terminal: if a model's output or a README could inject script into the page, it could type into your shell. So the interface never renders HTML from content, and every path the interface sends is canonicalised and checked against the project root in Rust.

## Agents write in a worktree, not in your project

An agent run creates a detached Git worktree of the project's `HEAD` inside NEXUS's data folder. The Developer step can only write there, through a path check that refuses `..`, absolute paths, anything under `.git`, and any symlink along the way. When you approve the final diff, NEXUS computes the patch once — *before* asking — and applies exactly that patch with `git apply --check` first, so it is all-or-nothing and refuses to clash with edits you made meanwhile.

Two surprises from the audit:

- `git worktree add` runs the repository's `post-checkout` hook. Starting an agent task should not run your hooks, so NEXUS passes `-c core.hooksPath=/dev/null` for that one command.
- NEXUS runs `git status` automatically when it shows a project. A repository's own `.git/config` can name a `core.fsmonitor` program, an external diff or a text-conversion filter — and an archive or a synced folder can carry that config. Every NEXUS Git call now disables those (`-c core.fsmonitor=false`, `--no-ext-diff --no-textconv`). The regression test configures all three to write a marker file and also runs plain Git as a control, proving the test would catch the problem.

## Sandboxing that says what it does

Approved commands run at one of three levels: **Trusted** (as you), **Restricted** and **Isolated**. On macOS the sandboxed levels generate a Seatbelt profile and run the command through `sandbox-exec`; on Linux they use bubblewrap.

The Restricted profile denies writes outside the working folder, temp and tool caches; denies reads of `~/.ssh`, `~/.aws`, `~/.kube`, `~/.docker`, `~/.netrc`, Git credentials, NEXUS's own database and more; and blocks the keychain by denying the Mach services it uses (`com.apple.SecurityServer`). Isolated also cuts the network.

One rule nearly shipped wrong. To let Isolated commands still reach servers on `localhost`, the first profile allowed `(allow network* (local ip "localhost:*"))`. The test that tries to connect to `192.0.2.1` — a reserved address that never answers — timed out instead of being refused: the rule had opened *all* outbound connections, apparently because the local address is not bound yet when an outgoing connect is checked. The fix allows only `network-bind` and `network-inbound` locally. Without a test that distinguishes "refused at once" from "timed out", this would have looked fine.

Two design choices:

- **Refuse, don't downgrade.** If a level cannot be enforced (Windows, or Linux without bubblewrap), it is refused, never quietly run as Trusted.
- **Say it in the record.** The first line of every command's output is the protection it actually ran with, its time limit and how many environment variables were withheld.

## Stopping a command means stopping everything it started

Cancel and timeout share one routine: SIGTERM to the process group and to every descendant found by walking parent PIDs, a two-second grace, then SIGKILL. The descendant walk catches children that called `setsid()` to leave the group. Processes that were in the first snapshot but have since been re-parented to `init` are killed too — and only those, so a PID reused by an unrelated process (which has a living parent) is never hit. Running commands are recorded on disk, so if NEXUS itself is killed, the next start stops what it left behind.

## A deadlock found by looking at the app, not the tests

While checking the UI, the app froze when a task step started waiting for approval. `sample` on the hung process showed the main thread waiting for the database mutex in `get_task`, and a Tokio worker holding that mutex while calling `window.is_focused()` to decide whether to show a desktop notification. `is_focused` dispatches to the main thread and waits. Each side waited for the other. The fix is one line — send the notification after releasing the lock — plus a warning on the helper. The unit tests could not have caught it: on Tauri's mock runtime, focus checks do not block.

## Credentials: masked for questions, placeholders for edits

Before source excerpts go to a model, a detector finds credential *values*: private-key bodies, vendor token formats, passwords in `scheme://user:password@host`, and string values assigned to secret-named identifiers (including typed ones like `password: &str = "…"`), while leaving `process.env.X`, `${X}` and placeholders like `your-api-key` alone.

For questions, the values become `[redacted]`. For the Developer, that would corrupt the code it rewrites, so NEXUS uses a vault: each value becomes a per-run placeholder (`NEXUS_SECRET_1_<random tag>`), the model is told to copy placeholders exactly, and the real values are restored in the files written. A placeholder the vault never issued — invented, or copied from another run — fails the step instead of being written.

## A report that cannot leak code

The codebase report is built only from the analysis and the index: counts, paths, module and package names, finding locations. Findings that quote a line (code markers) are reduced to their location. Everything is HTML-escaped, there are no scripts, and the file carries `default-src 'none'`. The test plants code in the fixture and in a finding's message, then asserts none of it appears in the output.

The graph inside it was the fiddly part. A naive "importers above what they import" layering explodes when modules form a cycle — each pushes the other up a layer until a cap. Grouping modules into strongly connected components first (Kosaraju) and layering the resulting acyclic graph puts every cycle on one row and keeps the picture compact.

## What I would do differently

Start the audit earlier. Most of the real bugs — the Git config execution, the worktree hook, the loopback rule, the deadlock — were found by writing a test that tried to break a specific promise, or by watching the real app, not by reading code.

NEXUS is MIT-licensed: https://github.com/yazhiniyanMS/nexus.
