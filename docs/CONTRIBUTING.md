# Contributing

Thanks for looking. NEXUS is a one-person project so far, and help is welcome — especially:

- **Trying it on Windows or Linux** and reporting what breaks (these builds have had little hands-on use).
- **Bug reports** with steps to reproduce (use **About & feedback** for a diagnostic report).
- **Index support for another language** (`src-tauri/src/intel/extract.rs` and `resolve.rs`).
- **Documentation** fixes where the docs and the app disagree.

Good first contributions are labelled [`good first issue`](https://github.com/yazhiniyanMS/nexus/labels/good%20first%20issue). If you want to work on something bigger, open an issue first so we can agree on the approach.

## Where to start

| You have | A good first contribution | Needs |
| --- | --- | --- |
| 20 minutes, no setup | Install a release on Windows or Linux and send a [platform report](https://github.com/yazhiniyanMS/nexus/issues/new?template=platform_report.yml) | Nothing but the installer |
| An hour | Fix a place where the docs and the app disagree, or a confusing message in the interface | Node; Rust only to run the app |
| An evening or two | A [`good first issue`](https://github.com/yazhiniyanMS/nexus/labels/good%20first%20issue) — for example remembering the map level per project (TypeScript only) | The full setup below |
| A weekend | Index support for another language in `src-tauri/src/intel/` (Java and C# are the largest gaps) | Rust |

Every issue and pull request gets a reply within a week. If you are unsure, comment on the issue before you start, so two people don't do the same work.

## Get the code

```bash
git clone https://github.com/yazhiniyanMS/nexus.git
cd nexus
```

## Set up

You need Node 24, Rust (the version in `rust-toolchain.toml` is installed automatically by `rustup`), Git, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your system.

On Ubuntu 22.04:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev libssl-dev pkg-config build-essential
```

On Windows: the "Desktop development with C++" workload from Visual Studio Build Tools, and the WebView2 runtime (already present on Windows 11).

```bash
npm install
npm run tauri dev          # run the app
```

## Checks to run before a pull request

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run typecheck
npm test
npm run build
```

`npm run tauri build -- --bundles app` (macOS) builds an installable app; installers for every system come from the release workflow ([RELEASES.md](RELEASES.md)).

The pull-request workflow runs these checks on Windows and Linux (Linux with bubblewrap, so the sandbox tests run), a macOS job for the execution and sandbox tests, and a dependency job (`cargo audit`, `npm audit`).

## How the code is organised

| Where | What |
| --- | --- |
| `src/views/` | One file (or folder) per screen |
| `src/components/ui.tsx` | Shared controls: buttons, status, tabs, headers, confirmations |
| `src/lib/api.ts` | The only place the interface calls the Rust core |
| `src-tauri/src/lib.rs` | Start-up and the list of commands the interface may call |
| `src-tauri/src/projects.rs`, `intel/` | Analysis, code index, graphs, the codebase report |
| `src-tauri/src/agents/` | Agent pipeline, roles and permissions, isolated workspaces |
| `src-tauri/src/jobs.rs`, `exec/` | Running approved commands: levels, environment, sandbox, limits |
| `src-tauri/src/terminal.rs` | Pseudo-terminals |
| `src-tauri/src/lab/` | Model Lab |
| `src-tauri/src/platform/` | Everything that differs between macOS, Windows and Linux |

More in [ARCHITECTURE.md](ARCHITECTURE.md); the interface rules are in [DESIGN.md](DESIGN.md).

## Sending a change

1. Fork, create a branch, make the change with a test that exercises it.
2. Run the checks above.
3. Open a pull request; the template asks what you checked. Small, focused pull requests are reviewed fastest.

## Working on another system's code from macOS

Windows-only code can be compile-checked on a Mac with a cross-compiler. It cannot be run this way.

```bash
brew install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo check --tests --target x86_64-pc-windows-gnu --manifest-path src-tauri/Cargo.toml
```

## Rules of the codebase

- **Platform differences live in `src-tauri/src/platform/`** (shells, processes, paths, search path, programs, secure storage, disk, notifications) and in `src/lib/platform.ts` for the UI. Feature modules do not check the operating system themselves.
- **Colours, radii and fonts come from the tokens in `src/index.css`.** Use `accent` (mint), `lav` (lavender), `ok`, `warn`, `danger` and the surface tokens; do not add palette classes such as `text-green-400` or hex colours in components.
- **Nothing runs without the user's approval.** Commands go through `jobs::run_command` with an execution policy (`exec::Policy`) after an approval gate; only the Terminal screen writes to a terminal.
- **The webview has no file or shell access.** Add a typed command in Rust and call it through `src/lib/api.ts`.
- **Schema changes are new migrations** in `db.rs` with an `up` and a `down`; never edit a released migration.
- **No network requests the user did not ask for**, and no analytics. See [PRIVACY.md](PRIVACY.md).
- Tests that need a Unix shell are marked `#[cfg(unix)]`; add a Windows counterpart where behaviour differs.

## Testing the real window

Debug builds started with `NEXUS_E2E_PORT=<port>` accept JavaScript on a loopback socket. Combine it with `NEXUS_DATA_DIR=<empty folder>` so tests never touch your own NEXUS data. Send exact text to terminals; never simulate individual key presses in a real shell.

By contributing you agree that your contribution is licensed under the [MIT licence](../LICENSE), and to follow the [code of conduct](../CODE_OF_CONDUCT.md).
