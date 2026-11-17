# NEXUS on macOS

macOS is the reference platform: NEXUS is developed and used on Apple Silicon.

| Area | Behaviour on macOS |
| --- | --- |
| Builds | Apple Silicon (`aarch64`) and Intel (`x64`) `.dmg`. The Intel build is cross-compiled and packaged, and has been launched under Rosetta on Apple Silicon; it has not been run on an Intel Mac. |
| Terminal | `openpty` pseudo-terminals running your login shell (`$SHELL`, usually zsh) with `-l`. Copy and paste are `⌘C` / `⌘V`. |
| App shortcuts | `⌘K` command palette, `⌘1…9` screens. |
| Search path | Apps started from Finder get a short `PATH`. NEXUS asks your login shell for its `PATH` once at start so approved commands find Homebrew, cargo and Node. |
| Git | `/usr/bin/git` (Xcode Command Line Tools) or Homebrew's. |
| API keys | The login Keychain, under `dev.nexus.workspace`. |
| Data folder | `~/Library/Application Support/dev.nexus.workspace`. |
| Notifications | A notification is shown when a task or agent step needs approval while the window is in the background. macOS asks for permission the first time. |

## Unsigned beta

The beta is not signed with a Developer ID and not notarized, so Gatekeeper blocks the first launch. Right-click the app and choose **Open**, or allow it under System Settings → Privacy & Security. Signing and notarization are prepared in the release workflow and switch on when the project has an Apple Developer certificate ([RELEASES.md](RELEASES.md#signing)).

## Known limitations

- macOS 12 or later.
- No automatic updates in the beta.
