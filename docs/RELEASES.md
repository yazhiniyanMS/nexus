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

What has actually been checked for 0.1.0. The four columns are different things and are not interchangeable.

| Platform | Compiled | Automated tests | Installed and launched | GUI verified by a person |
| --- | --- | --- | --- | --- |
| macOS Apple Silicon | Yes (local) | Yes (local, 151 Rust and 21 frontend tests) | Yes (local: app copied out of the `.dmg` and launched by the launch check) | Partly: welcome, Intelligence, Git, Terminal, Model Lab runtimes, Agents overview and About dialog were looked at in a debug build after the redesign; the full feature suite was not rerun |
| macOS Intel | Yes (local, cross-compiled and packaged as `.dmg`) | No | Launched once under Rosetta on Apple Silicon by the launch check; never on an Intel Mac | No |
| Windows x64 | Type-checked only, on macOS with a cross-compiler (`x86_64-pc-windows-gnu`) | Pending first CI run | Pending first CI run | **No** |
| Linux x64 | Pending first CI run | Pending first CI run | Pending first CI run | **No** |

No Windows or Linux virtual machine was available during development; the only route to those systems is GitHub's native runners, which first run when the repository is pushed. Update this table from the workflow results before publishing a release.

## Upgrades

The database is migrated forward on start, one numbered step at a time, inside a transaction. A test builds a database at the previous schema version with data in it, opens it with the current code, and checks the data is still there. Upgrades that skip several versions run the same steps in order but are not separately tested. There is no downgrade path in the app.
