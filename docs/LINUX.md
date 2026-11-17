# NEXUS on Linux

Linux support is new in the 0.1.0 beta and is built and checked on **Ubuntu 22.04 (x86-64) only**. Other distributions may work but are untested; see [RELEASES.md](RELEASES.md#verification-status).

## Required system libraries

The `.deb` and `.rpm` declare these as dependencies; for the AppImage you need them yourself.

| Library | Debian / Ubuntu package | Fedora package |
| --- | --- | --- |
| WebKitGTK 4.1 | `libwebkit2gtk-4.1-0` | `webkit2gtk4.1` |
| GTK 3 | `libgtk-3-0` | `gtk3` |
| D-Bus | `libdbus-1-3` | `dbus-libs` |
| App indicator (optional) | `libayatana-appindicator3-1` | `libayatana-appindicator-gtk3` |

glibc 2.35 or later is required because the packages are built on Ubuntu 22.04. Distributions that only ship WebKitGTK 4.0 (for example Ubuntu 20.04, Debian 11) are not supported.

## How Linux differs inside NEXUS

| Area | Behaviour on Linux |
| --- | --- |
| Terminal | Pseudo-terminals through `openpty`. New terminals start your login shell (`$SHELL`, then the account database, then `/bin/sh`) as a login shell. |
| Copy and paste in the terminal | `Ctrl+Shift+C` and `Ctrl+Shift+V`. |
| Search path | Desktop launchers start apps with a short `PATH`. NEXUS asks your login shell for its `PATH` once at start, so approved commands find tools such as `cargo`, `npm` or `nvm`-managed Node. |
| Stopping processes | Commands run in their own process group and are stopped with a signal to the group; terminals are hung up, then killed, by session. |
| API keys | The Secret Service (GNOME Keyring, KWallet, KeePassXC). Without a running, unlocked service, saving a key fails with a message that says so; everything that needs no key still works. |
| File dialogs | GTK dialogs, or the desktop portal where the system provides one. |
| Data folder | `$XDG_DATA_HOME/dev.nexus.workspace`, usually `~/.local/share/dev.nexus.workspace`. |
| Desktop entry | The `.deb` and `.rpm` install a launcher entry and icon under the Development category. The AppImage does not integrate itself. |

## Known limitations

- Only x86-64 is built. ARM64 Linux is not built or tested.
- Tested on Ubuntu 22.04 with X11 through a virtual display in CI. Wayland, other desktops and other distributions have not been checked by us.
- On some NVIDIA and Wayland setups WebKitGTK shows a blank window; starting NEXUS with `WEBKIT_DISABLE_DMABUF_RENDERER=1` is the usual workaround.
- The AppImage needs FUSE 2 (`libfuse2`) or `--appimage-extract-and-run`.

## Reporting Linux problems

Use the **Linux compatibility** issue form with your distribution, desktop and display server, and the report from **About & feedback**. If NEXUS does not start, run it from a terminal and include what it prints.
