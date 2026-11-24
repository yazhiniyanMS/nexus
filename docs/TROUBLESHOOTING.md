# Troubleshooting

If none of this helps, open **About & feedback** in the sidebar: it has a diagnostic report (no code, no file names, no keys) you can copy into a [bug report](https://github.com/yazhiniyanMS/nexus/issues/new?template=bug_report.yml).

## Installing and starting

| Problem | What to do |
| --- | --- |
| macOS: "NEXUS can't be opened because Apple cannot check it" | The beta is unsigned. Right-click NEXUS → **Open** → **Open**, or System Settings → Privacy & Security → **Open Anyway**. Needed once. |
| Windows: "Windows protected your PC" | **More info → Run anyway**. The installer is unsigned. |
| Linux: the window is blank or the app does not start | Install WebKitGTK 4.1 and GTK 3; see [LINUX.md](LINUX.md). |
| "NEXUS cannot start: NEXUS could not read its database…" | The database file is damaged or not a NEXUS database. It has not been changed. Move it aside (data folder in [INSTALLATION.md](INSTALLATION.md#where-your-data-lives)) to start fresh, or put back a `nexus.db.before-v<N>` copy. |
| "This database was created by a newer version of NEXUS" | You started an older NEXUS after a newer one. Install the newer version again. |

## Projects and the code index

| Problem | What to do |
| --- | --- |
| "Project folder no longer exists" | The folder was moved or renamed. Remove the project and add the new location. |
| The map is empty or shows one big circle | Only Rust, TypeScript, JavaScript, Python and Go are indexed. For nested projects choose **Architecture map (three levels)**. Rebuild the index after large changes. |
| A symbol is missing | Extraction is line-based; unusual declarations can be missed. Search (⌘3) always finds the text. |
| Cannot remove a project | An agent task is still running in it. Cancel it in Agents first. |

## Models

| Problem | What to do |
| --- | --- |
| "Could not connect. Check the base URL and that the server is running." | Start the server (Ollama: `ollama serve`, or **Model Lab → Runtimes → Start**). Check the address and port in Settings → Model connections. |
| Ollama connects but shows 0 models | Your Ollama model store is empty. Download one in **Model Lab → Library → Add model** or with `ollama pull <name>`. |
| "The server redirected to another address" | NEXUS does not follow redirects to another server with your key. Use the final address as the base URL. |
| "The server's reply is larger than 32 MB" | The base URL points at something that is not a model API. |
| NEXUS asks before sending project content | The connection is not on this computer. Confirm for that request, or use a local model. |

## Tasks, agents and commands

| Problem | What to do |
| --- | --- |
| A command says it "is not available on this system" | Restricted and Isolated need the macOS sandbox, or bubblewrap on Linux (`sudo apt install bubblewrap`). On Windows choose Trusted in Settings → Execution. |
| A command cannot find a tool or a variable | Restricted and Isolated pass only an allow-list of environment variables. Add the variable under Settings → Execution → Environment variables you allow, or run the command Trusted. |
| A build fails writing to a cache at Isolated | Isolated allows writes only to the project and temp folders. Use Restricted for builds that download dependencies. |
| "Stopped the command: it ran longer than its time limit" | Raise the limit in Settings → Execution, or for that task (Tasks → time limit per step). Watch commands (`npm test -- --watch`) never finish on their own. |
| The agent's change "does not apply cleanly" | You edited the same files after the agent started. Nothing was changed; resume or start a new task. |
| An agent failed with "placeholder NEXUS did not give it" | The model wrote a credential placeholder it was not given. Nothing was written. Retry, or use a stronger model. |
