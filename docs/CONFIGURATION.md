# Configuration

Everything is configured in the app, in **Settings** (⌘, on macOS, Ctrl+, elsewhere). There are no config files to edit.

## Model connections

**Settings → Model connections.** A connection is a model server NEXUS can talk to.

| Kind | Base URL example | Key |
| --- | --- | --- |
| Ollama | `http://localhost:11434` | none |
| llama.cpp server | `http://localhost:8080` | none |
| OpenAI-compatible (LM Studio, vLLM, LiteLLM, a hosted gateway…) | `http://localhost:1234/v1` | optional |
| Anthropic | `https://api.anthropic.com` | required |
| OpenAI | `https://api.openai.com/v1` | required |

- **Test** checks the connection and lists its models.
- **Keys** go into the macOS Keychain, Windows Credential Manager or the Linux Secret Service. They are never written to the database or a file, never shown again, and removed from any error message.
- **Local or remote.** A connection whose address is this computer (`localhost`, `127.0.0.1`) is local. Before project content (source excerpts for a question, files for an agent) goes to a remote connection, NEXUS asks you to confirm it for that request.
- **Disabled** connections are kept but never contacted.
- Redirects are followed only to the same server, so a key is never carried somewhere else.

## Default models

**Settings → Model defaults** sets the connection and model used when a screen does not ask, and default generation settings (temperature, maximum tokens…). Every screen that uses a model also lets you pick one.

## Execution

**Settings → Execution** controls *how* approved commands run — task steps and agent test commands. Approval decides *whether* a command runs; this decides what it can touch.

| Level | What a command can do |
| --- | --- |
| **Trusted** | Runs as you, with your files and network. Credential-like environment variables are removed. Not a sandbox. |
| **Restricted** | Sandboxed. Writes only to the working folder, temporary folders and tool caches. Cannot read SSH, cloud, Git or registry credentials, NEXUS's own data, or the keychain. Network allowed. Environment limited to an allow-list. |
| **Isolated** | Restricted, plus no network (macOS: only this computer; Linux: none), no cache writes, package-manager settings hidden. |

- Restricted and Isolated use the **macOS sandbox**, or **bubblewrap** on Linux (`sudo apt install bubblewrap`). Where neither exists (Windows, or Linux without bubblewrap) they are shown as unavailable and refused — never run at a lower level.
- Defaults: task steps **Trusted** (you wrote them); agent commands **Restricted** where available.
- The approval card for each command shows its level and time limit, and the first line of its output records exactly how it ran.

| Limit | Default | Notes |
| --- | --- | --- |
| Task step time limit | 30 min | A task can set its own (Tasks → time limit per step) |
| Agent command time limit | 15 min | |
| Output kept per command | 200 KB | |
| Largest file written | 4 GB | Restricted and Isolated |
| Memory per command | 8 GB | Isolated on Linux only; macOS cannot enforce it |
| Agent tasks at once / queued | 2 / 20 | |
| Task runs at once | 4 | |

When a limit passes, the command and everything it started are stopped, and the record says why.

### Environment variables

Trusted commands get your environment minus anything that looks like a credential (names such as `*_TOKEN`, `*_API_KEY`, `*_PASSWORD`, and any value that is a URL with a password or a known token format). Restricted and Isolated commands get only what tools need to find themselves (`PATH`, `HOME`, locale, toolchain folders such as `CARGO_HOME` or `JAVA_HOME`).

If a task really needs a variable — for example `GITHUB_TOKEN` for `gh` — add its name under **Environment variables you allow**. It is then passed at every level.

## General

**Settings → General**:

- **Open NEXUS on** — Overview, or the screen you used last.
- **Data folder** — where the database, terminal output, agent workspaces and downloaded models live. To use another folder, start NEXUS with the environment variable `NEXUS_DATA_DIR` set to an absolute path.
- **Show the welcome again** on the next start.

## Database backups

Before NEXUS upgrades its database to a new version, it writes a copy next to it (`nexus.db.before-v<N>`). A database from a newer NEXUS is refused rather than opened. See [INSTALLATION.md](INSTALLATION.md#where-your-data-lives) for the folder.
