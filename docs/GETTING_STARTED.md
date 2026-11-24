# Getting started

From download to a mapped project in about five minutes. No account and no AI key needed.

## 1. Install

Download the installer for your system from [Releases](https://github.com/yazhiniyanMS/nexus/releases) and follow [INSTALLATION.md](INSTALLATION.md). The beta builds are not code-signed, so the first launch needs one extra click (macOS: right-click → **Open**; Windows: **More info → Run anyway**).

Optional but useful:

- **Git** — for history, diffs, commits and agents. NEXUS tells you if it cannot find it.
- **Ollama** ([ollama.com](https://ollama.com)) or a llama.cpp server — for asking questions and running agents with a model on your own computer.

## 2. First run

NEXUS opens with a short setup. Every step can be skipped.

1. **Welcome** — what NEXUS does and what stays on your computer.
2. **Your project** — choose a folder with code in it. NEXUS analyses it (languages, frameworks, dependencies, findings) and builds a code index of its symbols and imports. Your files are never moved or changed.
3. **A model (optional)** — if Ollama is running, connect and test it with one click; if it is installed but not running, NEXUS can start it (bound to this computer only). Hosted providers are set up later in **Settings → Model connections**.
4. **Start** — suggestions for what to try first.

To see the setup again: **Settings → General → Show it again on next start**.

## 3. Five things to try

| Try | Where | Needs a model |
| --- | --- | --- |
| See the architecture: modules, imports, cycles | **Intelligence → Map** (try "three levels" for nested projects) | No |
| Share a codebase report with no source code in it | **Overview → Share a report** | No |
| Find where something is defined and used | **Intelligence → Symbols**, or **⌘K** and type a name | No |
| Review and commit what you changed | **Git** (⌘4) — stage files, write a message, ⌘Enter | No |
| Ask how something works, with cited sources | **Intelligence → Ask** | Yes |
| Let an agent make a small change | **Agents → New** — approve the plan, each command and the final diff | Yes |

## 4. What next

- Every screen: [USER_GUIDE.md](USER_GUIDE.md)
- Models, keys, sandboxing and limits: [CONFIGURATION.md](CONFIGURATION.md)
- Something not working: [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- Report a bug or ask for a feature: **About & feedback** in the sidebar (it includes a diagnostic report you can read before sending)
