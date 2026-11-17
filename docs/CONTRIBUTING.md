# Contributing

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
cargo test --manifest-path src-tauri/Cargo.toml
npm run typecheck
npm test
npm run build
```

The pull-request workflow runs the same checks on macOS, Windows and Linux.

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
- **Nothing runs without the user's approval.** Commands go through `jobs::run_command` after an approval gate; only the Terminal screen writes to a terminal.
- **The webview has no file or shell access.** Add a typed command in Rust and call it through `src/lib/api.ts`.
- **Schema changes are new migrations** in `db.rs` with an `up` and a `down`; never edit a released migration.
- **No network requests the user did not ask for**, and no analytics. See [PRIVACY.md](PRIVACY.md).
- Tests that need a Unix shell are marked `#[cfg(unix)]`; add a Windows counterpart where behaviour differs.

## Testing the real window

Debug builds started with `NEXUS_E2E_PORT=<port>` accept JavaScript on a loopback socket. Combine it with `NEXUS_DATA_DIR=<empty folder>` so tests never touch your own NEXUS data. Send exact text to terminals; never simulate individual key presses in a real shell.
