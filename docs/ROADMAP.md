# Roadmap

Ordered by value to people using NEXUS, not by how interesting a feature is to build. Dates are not promised; this is a one-person project.

## Now — 0.2.0

The work is done in `main`; it needs a verified release.

- [x] Codebase report: a shareable HTML file with no source code
- [x] Guided first run: add a project, index it, connect a local model, next steps
- [x] Execution levels (Trusted, Restricted, Isolated) with the macOS sandbox and bubblewrap; time limits; environment policy; process-tree cleanup
- [x] Credentials sent to models as placeholders in agent edits; masked in questions
- [x] Commit from the Git screen; merge and conflict state
- [x] Redesigned interface; Settings screen
- [ ] CI green on Windows and Linux for all of the above, then tag 0.2.0
- [ ] Hands-on check of the 0.2.0 installers on Windows and Linux

## Next

- **Trust on every platform:** a Windows sandbox backend (restricted token or AppContainer, job objects for memory and process limits).
- **Installers people do not have to click through:** signed and notarized macOS builds, signed Windows installers (needs certificates).
- **Faster, fresher index:** incremental re-indexing on file changes.
- **Agent quality with local models:** smaller, checkable steps; better prompts for 7–30B models; clearer failure explanations.
- **Report improvements:** compare two reports (what changed between versions); optional Markdown export of the same content.

## Later

- More languages in the index (Java, C#, Kotlin, Swift, C/C++), ideally with tree-sitter.
- Homebrew cask and a winget package, once releases are signed.
- An isolated level with a private home folder, so reads of the rest of your home are blocked too.

## Not planned

- **An editor.** Good editors exist; NEXUS works next to them.
- **Telemetry or accounts.** If optional analytics are ever proposed they will be off by default and documented first ([PRIVACY.md](PRIVACY.md)).
- **Bundling a model or a hosted service.**

Suggestions: [open a feature request](https://github.com/yazhiniyanMS/nexus/issues/new?template=feature_request.yml).
