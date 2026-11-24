## What this changes

<!-- One or two sentences. Link the issue if there is one: "Fixes #123". -->

## Why

<!-- The problem it solves for someone using NEXUS. -->

## How it was checked

<!-- Tick what you ran; say what you could not check. -->

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `npm run typecheck && npm test && npm run build`
- [ ] Tried it in the running app (`npm run tauri dev`) on: <!-- macOS / Windows / Linux -->

## Checklist

- [ ] Nothing runs, writes or sends data without the user asking (see [PRIVACY.md](../docs/PRIVACY.md))
- [ ] New behaviour has a test that exercises it
- [ ] Docs updated if the user-visible behaviour changed
- [ ] Screenshots attached for interface changes
