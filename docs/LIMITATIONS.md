# Known limitations

NEXUS is beta software made by one student developer. This page lists what it does not do, so you do not have to find out the hard way. Security-specific residual risks are in [audit/SECURITY_AUDIT.md](audit/SECURITY_AUDIT.md#remaining-risks-after-security-20).

## Scope

- **Not an editor.** Files open in a read-only viewer. Edit in your usual editor; NEXUS sees the changes.
- **Git:** no push, pull, fetch, branch switching, discarding changes or conflict resolution inside the app (conflicts are resolved in your editor, then marked resolved here).
- **No automatic updates** and **no signed installers** yet.

## Code intelligence

- The index covers Rust, TypeScript, JavaScript, Python and Go. Java, C#, C/C++, Kotlin and Swift are not indexed (they still appear in search and analysis).
- Symbols and imports are extracted with line-based rules, not a full parser, so unusual constructs can be missed; references are matched by name, not by scope.
- The index is rebuilt on request; there is no file watcher. A project is capped at 60 000 files; text files over 1.5 MB are not indexed (search reads files up to 64 MB).

## Agents

- Need a Git repository with at least one commit, and a model. Small local models often produce plans or changes that fail validation; larger models work better.
- The Developer returns whole files and cannot delete files. At most 40 files per change, 400 KB each.
- Pause takes effect before the next step, not in the middle of one.
- Repository text (READMEs, comments) can steer what a model proposes. Approvals, the isolated worktree and the execution level limit the effect; they cannot make a model's suggestions correct.

## Execution and sandboxing

- **Windows has no sandbox backend**: commands there run Trusted only. Linux needs bubblewrap for Restricted and Isolated.
- Restricted allows network access and can read most of your home folder except the hidden credential locations; use Isolated for code you do not trust.
- Memory limits are enforced only on Linux (Isolated). macOS's `sandbox-exec` is deprecated by Apple; NEXUS checks that it works at start-up and refuses the sandboxed levels if it stops working.
- Credential detection (for masking and placeholders) is pattern-based: a secret with no recognisable format under an ordinary name is not caught.

## Terminal

- Shells end when NEXUS closes (their output is kept). Saved output is limited to about 1–2 MB per terminal and is replayed at the current width, so old full-screen output can look different. Output search works on the raw stream, so redrawn lines can appear twice.

## Model Lab

- Memory use and load time are reported only by Ollama; OpenAI-compatible servers report model names only.
- NEXUS starts Ollama and llama.cpp servers but does not install runtimes. Hugging Face downloads are limited to public, non-gated repositories.

## Platforms

- macOS on Apple Silicon is used every day. Intel macOS, Windows and Linux builds are built, installed and launch-checked in CI, but have had little hands-on use. Reports from those systems are especially welcome.
- Linux needs a running Secret Service (GNOME Keyring, KWallet) to store API keys.
