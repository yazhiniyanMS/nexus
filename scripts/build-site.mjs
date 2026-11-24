// Builds the public website from website/ into a folder: fills in the repository address, the
// site address and the version, copies the screenshots and demo files from docs/assets (one copy
// in the repository), and writes sitemap.xml and robots.txt.
// Usage: node scripts/build-site.mjs --repo owner/name [--site https://…/] [--out _site]
// The repository is always passed in (CI uses its own), so no address is hard-coded in the pages.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** The GitHub Pages address for a repository: https://owner.github.io/name/ */
export function pagesUrl(repo) {
  const [owner, name] = repo.split("/");
  return `https://${owner.toLowerCase()}.github.io/${name}/`;
}

export function render(html, { repo, version, site }) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo)) throw new Error(`--repo must look like owner/name, got "${repo}"`);
  const siteUrl = site ?? pagesUrl(repo);
  if (!/^https:\/\/[^\s"<>]+\/$/.test(siteUrl)) throw new Error(`--site must be an https address ending in /, got "${siteUrl}"`);
  const repoUrl = `https://github.com/${repo}`;
  const out = html
    .replaceAll("{{REPO_URL}}", repoUrl)
    .replaceAll("{{RELEASES_URL}}", `${repoUrl}/releases`)
    .replaceAll("{{SITE_URL}}", siteUrl)
    .replaceAll("{{VERSION}}", version);
  const left = out.match(/\{\{[A-Z_]+\}\}/);
  if (left) throw new Error(`Unfilled placeholder ${left[0]}`);
  return out;
}

export function sitemap(siteUrl) {
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n  <url><loc>${siteUrl}</loc></url>\n</urlset>\n`;
}

export function robots(siteUrl) {
  return `User-agent: *\nAllow: /\nSitemap: ${siteUrl}sitemap.xml\n`;
}

/** Files the page uses, copied from docs/assets into the site's assets folder. */
export const ASSETS = ["screenshots", "demo", "social-preview.png"];

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  const arg = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const repo = arg("--repo");
  const out = arg("--out", "_site");
  if (!repo) {
    console.error("usage: build-site.mjs --repo owner/name [--site https://…/] [--out dir]");
    process.exit(2);
  }
  const site = arg("--site", pagesUrl(repo));
  const version = JSON.parse(readFileSync("package.json", "utf8")).version;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(join(out, "assets"), { recursive: true });
  cpSync("website", out, { recursive: true });
  for (const item of ASSETS) cpSync(join("docs", "assets", item), join(out, "assets", item), { recursive: true });
  writeFileSync(join(out, "index.html"), render(readFileSync("website/index.html", "utf8"), { repo, version, site }));
  writeFileSync(join(out, "sitemap.xml"), sitemap(site));
  writeFileSync(join(out, "robots.txt"), robots(site));
  console.log(`Site for ${repo} v${version} at ${site} written to ${out}`);
}
