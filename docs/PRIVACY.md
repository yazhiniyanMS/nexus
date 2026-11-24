# Privacy

NEXUS is local-first. This page lists everything that can leave your computer and when.

## What NEXUS never does

- No account, sign-in or licence check.
- No analytics, telemetry, usage statistics or crash uploads. There is no code for any of them in the app.
- No background network activity. NEXUS does not check for updates.
- It does not upload source code, the code index, terminal output, project names, settings or credentials on its own.

## What is stored, and where

| Data | Where |
| --- | --- |
| Project list, analysis, code index, tasks, agent history, benchmark results, settings | `nexus.db` in the data folder ([INSTALLATION.md](INSTALLATION.md#where-your-data-lives)) |
| Terminal scrollback (about 1–2 MB per terminal) | `terminal/` in the data folder |
| Agent workspaces (isolated copies of a repository) | `workspaces/` in the data folder, removed when a task ends |
| Model files you download | The model folder you choose in Model Lab |
| API keys | macOS Keychain, Windows Credential Manager or the Linux Secret Service. Never the database or a file. |

## When data leaves your machine

Only as a direct result of something you do:

| You do this | This is sent | To |
| --- | --- | --- |
| Send a prompt, ask a codebase question with an AI answer, or run an agent, using a **remote** provider you configured | The prompt. For codebase questions and agents this includes the source excerpts the answer is based on. Recognisable credentials in those excerpts are masked; in files an agent edits they are replaced by placeholders and restored only on your machine. | That provider's address. The screen says when a provider is not on this machine and asks you to confirm before anything is sent to it; NEXUS refuses to send project content to a remote provider without that confirmation. |
| Use a **local** runtime (Ollama, llama.cpp, LM Studio on `localhost`) | The same content | Only your own machine |
| Search for or download a model in Model Lab | Your search text, then the file request | Hugging Face, or the address you typed, or your Ollama server |
| Click **Report a bug**, **Request a feature** or **Documentation** | Nothing from NEXUS; your browser opens a GitHub page | github.com |
| Let a tool you run in the terminal or an approved task use the network | Whatever that tool sends | Wherever it sends it. Terminals and Trusted commands are not sandboxed; Isolated commands have no network ([CONFIGURATION.md](CONFIGURATION.md#execution)). |
| Save a **codebase report** | Nothing; the file is written where you choose | Your disk. It contains file paths, module and package names and counts, but no source code. |

## The diagnostic report

**About & feedback** shows a report with: the NEXUS version, operating system and version, CPU model, core count and memory size, shell name, Git version, the name of the secure storage in use, the data folder (with your home folder written as `~`), the database schema number, and counts of projects, terminals, agent tasks and AI connections by type.

It never contains source code, terminal output, file or project names, provider addresses, API keys or environment variables. Credential-looking text is masked. You see the whole report, can edit it, and it goes nowhere unless you copy or save it and send it yourself.

## Analytics in future versions

If optional analytics are ever added they will be off by default, need an explicit opt-in, and be documented here first. No feature will depend on them.
