# NEXUS on Windows

Windows support is new in the 0.1.0 beta. See [RELEASES.md](RELEASES.md#verification-status) for exactly what has and has not been tested.

## How Windows differs inside NEXUS

| Area | Behaviour on Windows |
| --- | --- |
| Terminal | Real pseudo-terminals through ConPTY (needs Windows 10 1809 or later). New terminals start PowerShell 7 (`pwsh`) if installed, otherwise Windows PowerShell, otherwise Command Prompt. The shell menu next to **New terminal** lists every shell found. |
| Copy and paste in the terminal | `Ctrl+Shift+C` and `Ctrl+Shift+V`. `Ctrl+C` stays the interrupt key. |
| App shortcuts | `Ctrl+K` (or `Ctrl+Shift+P`) opens the command palette, `Ctrl+1…9` switch screens. Inside a terminal, `Ctrl` combinations go to the shell, so use `Ctrl+Shift+P` there. |
| Task and agent commands | Run through `cmd.exe /D /S /C` after you approve them, with UTF-8 output. Write commands in Command Prompt syntax. |
| Stopping processes | Windows has no signals or process groups. NEXUS ends a command, a terminal or a model server with `taskkill /T`, which stops the whole process tree. Programs get no chance to clean up when a terminal is stopped. |
| Git | `git.exe` is found on `PATH`, then in the usual Git for Windows locations (`%ProgramFiles%\Git\cmd`, `%LOCALAPPDATA%\Programs\Git\cmd`). Install [Git for Windows](https://git-scm.com/download/win) if it is missing. |
| Paths | Stored without the `\\?\` prefix. Paths inside a project are shown with forward slashes. |
| API keys | Windows Credential Manager, under `dev.nexus.workspace`. |
| Data folder | `%APPDATA%\dev.nexus.workspace`. |
| Disk space | Read with `GetDiskFreeSpaceExW`. |

## Known limitations

- The installers are unsigned, so SmartScreen warns on first run.
- Only x64 is built. ARM64 Windows is not built or tested.
- Model Lab's hardware line shows the processor identifier but not installed memory.
- Agent test commands suggested by a model may assume Unix tools; the planning agent is told commands run in `cmd.exe`, but review each command before approving it.
- Symbolic links in a project need Developer Mode or administrator rights on Windows, as with any tool.

## Reporting Windows problems

Use the **Windows compatibility** issue form and include the report from **About & feedback**.
