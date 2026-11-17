// Builds the public website from website/ into a folder, filling in the repository address and
// the version. Usage: node scripts/build-site.mjs --repo owner/name [--out _site]
// The repository is always passed in (CI uses its own), so no address is hard-coded in the pages.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export function render(html, { repo, version }) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo)) throw new Error(`--repo must look like owner/name, got "${repo}"`);
  const repoUrl = `https://github.com/${repo}`;
  const out = html.replaceAll("{{REPO_URL}}", repoUrl).replaceAll("{{RELEASES_URL}}", `${repoUrl}/releases`).replaceAll("{{VERSION}}", version);
  const left = out.match(/\{\{[A-Z_]+\}\}/);
  if (left) throw new Error(`Unfilled placeholder ${left[0]}`);
  return out;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  const arg = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const repo = arg("--repo");
  const out = arg("--out", "_site");
  if (!repo) {
    console.error("usage: build-site.mjs --repo owner/name [--out dir]");
    process.exit(2);
  }
  const version = JSON.parse(readFileSync("package.json", "utf8")).version;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  cpSync("website", out, { recursive: true });
  writeFileSync(join(out, "index.html"), render(readFileSync("website/index.html", "utf8"), { repo, version }));
  console.log(`Site for ${repo} v${version} written to ${out}`);
}
