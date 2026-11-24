# Installing NEXUS

NEXUS is in public beta. Installers are attached to each release on GitHub together with `SHA256SUMS.txt` and `release-manifest.json`. You do not need Rust, Node or the source code to install it.

**The beta installers are not code-signed or notarized.** macOS and Windows will warn you on first launch; the steps below say what to expect. Always compare your download's SHA-256 with `SHA256SUMS.txt`.

## System requirements

| System | Requirement |
| --- | --- |
| macOS | macOS 12 or later, Apple Silicon or Intel |
| Windows | Windows 10 (1809 or later) or Windows 11, 64-bit, with the WebView2 runtime (part of Windows 11; the installer fetches it on Windows 10 if missing) |
| Linux | x86-64, glibc 2.35 or later, WebKitGTK 4.1 and GTK 3 (see [LINUX.md](LINUX.md)) |
| All | About 200 MB of disk space. Git is needed for Git features and agents. An AI provider or local model runtime is optional. |

## Checking your download

```bash
shasum -a 256 NEXUS_0.1.0_aarch64.dmg        # macOS
sha256sum NEXUS_0.1.0_amd64.deb              # Linux
```

```powershell
Get-FileHash .\NEXUS_0.1.0_x64-setup.exe -Algorithm SHA256   # Windows
```

The value must match the line for that file in `SHA256SUMS.txt`.

## macOS

1. Open the `.dmg` (`aarch64` for Apple Silicon, `x64` for Intel) and drag **NEXUS** to **Applications**.
2. First launch: open NEXUS; macOS blocks it because the beta is unsigned. Go to **System Settings → Privacy & Security**, scroll to Security, click **Open Anyway**, then **Open**, and enter your password. Needed once. (Since macOS 15, right-click → Open no longer bypasses this; on macOS 14 and earlier it still works.)

More in [MACOS.md](MACOS.md).

## Windows

1. Run `NEXUS_<version>_x64-setup.exe` (or the `.msi` if your organisation prefers MSI).
2. SmartScreen shows "Windows protected your PC" because the installer is unsigned: choose **More info**, then **Run anyway**.
3. The `-setup.exe` installs for the current user and needs no administrator rights.

More in [WINDOWS.md](WINDOWS.md).

## Linux

```bash
sudo apt install ./NEXUS_<version>_amd64.deb          # Debian, Ubuntu
sudo dnf install ./NEXUS-<version>-1.x86_64.rpm       # Fedora, RHEL
chmod +x NEXUS_<version>_amd64.AppImage && ./NEXUS_<version>_amd64.AppImage   # anything else
```

More in [LINUX.md](LINUX.md).

## Where your data lives

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/dev.nexus.workspace` |
| Windows | `%APPDATA%\dev.nexus.workspace` |
| Linux | `~/.local/share/dev.nexus.workspace` (or `$XDG_DATA_HOME/dev.nexus.workspace`) |

It holds the database (`nexus.db`), terminal scrollback, agent workspaces and downloaded model files. API keys are not in this folder; they are in your system's secure storage. Set the environment variable `NEXUS_DATA_DIR` to an absolute path to use a different folder (a second profile, or a portable copy).

## Updating

Install the new version over the old one. There is no automatic updater in the beta.

- Your data folder is kept. On start, NEXUS brings the database up to date step by step; existing projects, settings, model connections and history are preserved (an automated test upgrades a database from the previous schema version with data in it and checks nothing is lost).
- NEXUS never changes files in your project folders during an update.
- Downgrading to an older version after running a newer one is not supported: back up the data folder first if you may want to go back.

## Uninstalling

| System | How |
| --- | --- |
| macOS | Drag NEXUS from Applications to the Bin. |
| Windows | Settings → Apps → Installed apps → NEXUS → Uninstall. |
| Linux | `sudo apt remove nexus` or `sudo dnf remove nexus`; delete the AppImage file. |

Uninstalling leaves your data folder in place. To remove everything, delete the folder listed above and remove the entries named `dev.nexus.workspace` from Keychain Access (macOS), Credential Manager (Windows) or your keyring application (Linux).

## Building from source

See [CONTRIBUTING.md](CONTRIBUTING.md).
