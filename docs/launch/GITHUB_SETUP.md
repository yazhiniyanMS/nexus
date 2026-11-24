# GitHub setup — waiting for the owner's approval

These change the public repository, so none of them has been applied. Each is free and reversible. Run them from the repository folder with the GitHub CLI signed in as the owner, or use the web settings named after each step.

## 1. Description, homepage and topics

```bash
gh repo edit yazhiniyanMS/nexus \
  --description "Local-first desktop workspace to understand any codebase without AI — architecture map, cycles, hotspots, shareable report — and let AI agents change it only through approvals, in an isolated Git worktree with sandboxed commands. Works with Ollama and other local models. macOS, Windows, Linux. MIT." \
  --homepage "https://yazhiniyanms.github.io/nexus/" \
  --add-topic developer-tools --add-topic code-intelligence --add-topic codebase-visualization \
  --add-topic ai-agents --add-topic coding-agent --add-topic local-first --add-topic ollama \
  --add-topic llm --add-topic tauri --add-topic rust --add-topic react --add-topic desktop-app
```

Set the homepage only after step 2 has deployed the site.

## 2. Website (GitHub Pages)

The `Website` workflow failed on 17 November because Pages is not enabled. Enable it with GitHub Actions as the source:

```bash
gh api -X POST repos/yazhiniyanMS/nexus/pages -f build_type=workflow
```

(Web: Settings → Pages → Source: **GitHub Actions**.) The site will be at `https://yazhiniyanms.github.io/nexus/`.

Root cause of the failed `github-pages` deployment (17 November): the `Website` workflow ran when 0.1.0 was published, and `actions/configure-pages` found no Pages site for the repository (`GET /repos/…/pages` → 404). Nothing in the build was wrong; Pages had never been enabled. The API cannot be enabled from the workflow with its default token, so this one setting is the owner's.

The workflow now deploys **from `main` only** (on a push that changes the site, or by hand on `main`), stops with a clear message if Pages is still off, and pull requests only build and check the site. After enabling Pages, publish with:

```bash
gh workflow run pages.yml -R yazhiniyanMS/nexus --ref main
```

Do this after PR #1 is merged; the page states that the current download is 0.1.0 and the described features ship in 0.2.0.

## 3. Social preview

Upload `docs/assets/social-preview.png` (1280×640) in Settings → General → Social preview. There is no API for this.

## 4. Private vulnerability reporting

`SECURITY.md` points reporters to it:

```bash
gh api -X PUT repos/yazhiniyanMS/nexus/private-vulnerability-reporting
```

## 5. Discussions (optional)

Useful for questions and ideas that are not bugs; leave it off if you will not have time to answer.

```bash
gh repo edit yazhiniyanMS/nexus --enable-discussions
```

## 6. Labels

The repository already has `bug`, `enhancement`, `documentation`, `good first issue`, `help wanted` and `accessibility`. Two more help triage:

```bash
gh label create platform:windows --color 0e8a16 --description "Specific to Windows" -R yazhiniyanMS/nexus
gh label create platform:linux --color 0e8a16 --description "Specific to Linux" -R yazhiniyanMS/nexus
```

## 7. Release 0.2.0 (after CI passes)

Follow [RELEASES.md](../RELEASES.md): bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (checked by `scripts/check-version.mjs`), move the changelog's Unreleased section under 0.2.0, push, let the pull-request and main workflows pass on all systems, tag `v0.2.0`, review the draft release and its checksums, then publish.
