# Releases

## How a release is made

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (`node scripts/check-version.mjs` checks them). Versions are numeric `major.minor.patch` because MSI allows nothing else.
2. Add `docs/releases/v<version>.md` (release notes) and a `CHANGELOG.md` entry.
3. Merge to `main`. The **CI** workflow builds and packages every platform; its installers are workflow artifacts, not a release.
4. Push a tag `v<version>`. The **Release** workflow checks the tag against the version, builds every installer on native runners, writes `SHA256SUMS.txt` and `release-manifest.json`, and creates a **draft** GitHub release.
5. A maintainer reviews the draft and the workflow log, then publishes it by hand. Nothing becomes public without that step.
6. Publishing a release (or running the **Website** workflow by hand) deploys the website to GitHub Pages.

| Workflow | Trigger | Publishes? |
| --- | --- | --- |
| `pr.yml` | Pull requests | No. Format, tests, type-check and a full compile on Windows and Linux. |
| `ci.yml` | Push to `main`, or by hand | No. Full build, installers, install and launch checks on Windows and Linux; macOS too when run by hand with `platforms: macos,windows,linux`. Artifacts kept 14 days. |
| `release.yml` | Tag `vX.Y.Z` | Draft release only. |
| `pages.yml` | Release published, or by hand | The website. |

## What each build job does

Check out → system libraries (Linux) → Node and Rust → `npm ci` → `cargo fmt --check` → type-check → frontend tests → Rust tests → frontend build → `tauri build` → collect installers → **launch check** (start the built app with an empty data folder and confirm it stays up and creates its database) → **install check** (install the `.deb`, the NSIS installer or the `.dmg`, start the installed app, uninstall) → upload.

The launch and install checks start the real application, but nobody looks at the window. They are not GUI verification.

## Artifacts

| Platform | Files |
| --- | --- |
| macOS | `NEXUS_<v>_aarch64.dmg`, `NEXUS_<v>_x64.dmg` |
| Windows | `NEXUS_<v>_x64-setup.exe` (NSIS), `NEXUS_<v>_x64_en-US.msi` |
| Linux | `NEXUS_<v>_amd64.AppImage`, `NEXUS_<v>_amd64.deb`, `NEXUS-<v>-1.x86_64.rpm` |
| All | `SHA256SUMS.txt`, `release-manifest.json` (file, platform, architecture, size, SHA-256, and whether it was signed) |

## Signing

No signing credentials are configured, so **all installers are currently unsigned**. The manifest records `"signed": false` for each file and the documentation tells users what warnings to expect.

| Platform | Prepared | To turn on |
| --- | --- | --- |
| macOS | The build passes `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID` to the Tauri bundler when the `APPLE_CERTIFICATE` secret exists; the bundler then signs and notarizes. | Join the Apple Developer Program, export a Developer ID Application certificate as base64 `.p12`, add the six repository secrets. Untested until a certificate exists. |
| Windows | Not wired up. | Obtain an Authenticode certificate (or Azure Trusted Signing) and add `bundle.windows.signCommand` or a certificate thumbprint in a release-only config. |
| Linux | Checksums only. | Optionally sign `SHA256SUMS.txt` with a project GPG key. |

Never commit certificates, keys or passwords. Do not describe a build as signed or notarized unless the workflow log and `codesign -dv` / `spctl -a` (macOS) or `Get-AuthenticodeSignature` (Windows) confirm it.

## Verification status

What has actually been checked for 0.1.0. The columns are different kinds of evidence and are not interchangeable: "installed and launched" means a script installed the package, started the app with an empty data folder and confirmed it stayed up and created its database. Nobody looked at the window.

| Platform | Compiled and packaged | Automated tests | Installed and launched | Looked at by a person |
| --- | --- | --- | --- | --- |
| macOS Apple Silicon | Yes (local) | Yes (local, all Rust and frontend tests) | Yes (local: app copied out of the `.dmg`) | Partly: welcome, Intelligence, Git, Terminal, Model Lab runtimes, Agents overview and About dialog, in a debug build after the redesign |
| macOS Intel | Yes (local, cross-compiled `.dmg`) | No | Launched under Rosetta only | No |
| Windows x64 | Yes, CI (`windows-latest`): NSIS `-setup.exe` and `.msi` | Yes, CI: Rust and frontend tests (see coverage note) | Yes, CI: built binary launched; NSIS installer run silently, installed app launched, uninstaller run. The `.msi` was built but not installed. | **No** |
| Linux x64 | Yes, CI (`ubuntu-22.04`): `.deb`, `.rpm`, `.AppImage` | Yes, CI: Rust and frontend tests | Yes, CI: built binary and installed `.deb` launched under a virtual display, `.deb` removed again. The `.rpm` and `.AppImage` were built but not installed or run. | **No** |

Evidence: CI run [43999204319](https://github.com/yazhiniyanMS/nexus/actions/runs/43999204319) on commit `4a04aa9` (private repository); the coverage changes since then are in the next run on `main`.

### Coverage differences

- **Windows:** tests that drive real shells use Command Prompt or PowerShell (ConPTY terminals, exit codes, resize, stopping a running program, PowerShell input). Still Unix-only, because their fixtures are shell scripts: the end-to-end agent workflow tests (10), the managed model-server process tests (3), the executable-bit check for runtime binaries, the login-shell `PATH` test and three Unix terminal tests (background jobs, `stty` resize, search with colour codes). The agent code paths they cover (worktrees, diffs, apply, line endings) are partly covered on Windows by the workspace tests.
- **Linux:** every test that runs on macOS runs on Linux.

### Problems found by the first CI runs, and fixed

| Platform | Problem | Fix |
| --- | --- | --- |
| Windows, Linux | `node --test tests/` fails with Node 24 (folder taken as a module) | Test files passed as a glob |
| Windows | Rust test executable crashed on load (`STATUS_ENTRYPOINT_NOT_FOUND`): the Common Controls manifest was only in the app | Manifest embedded by the linker into every executable |
| Windows | Terminals produced no output: ConPTY waits for an answer to its start-up cursor query, and the screen ignores queries found in replayed output | The core answers that first query |
| Windows | `git worktree add` failed on a `\\?\C:\…` path | Git is given the plain path |
| Windows | Agent tests assumed LF checkouts; Git for Windows uses CRLF | Tests compare content; a new test checks CRLF checkouts keep CRLF after an apply |

### Known limits found in CI

- **Linux without a D-Bus session bus:** the app takes about 30 seconds to start (a D-Bus timeout); inside a session bus, as on any desktop, it starts in under a second. Affects minimal window managers or containers started without `dbus-run-session`.
- Windows SmartScreen and macOS Gatekeeper warnings: installers are unsigned.

## Upgrades

The database is migrated forward on start, one numbered step at a time, inside a transaction. A test builds a database at the previous schema version with data in it, opens it with the current code, and checks the data is still there. Upgrades that skip several versions run the same steps in order but are not separately tested. There is no downgrade path in the app.
