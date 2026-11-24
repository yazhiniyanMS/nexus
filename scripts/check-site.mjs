// Checks a built website folder: every local link and image resolves to a file, the pages carry
// the metadata search engines and link previews need, and nothing was left unfilled.
// Usage: node scripts/check-site.mjs [_site]
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { pathToFileURL } from "node:url";

export function checkSite(dir) {
  const problems = [];
  const pages = readdirSync(dir).filter((f) => f.endsWith(".html"));
  if (!pages.includes("index.html")) problems.push("index.html is missing");
  for (const required of ["robots.txt", "sitemap.xml", "styles.css"]) {
    if (!existsSync(join(dir, required))) problems.push(`${required} is missing`);
  }
  for (const page of pages) {
    const html = readFileSync(join(dir, page), "utf8");
    if (html.includes("{{")) problems.push(`${page}: unfilled placeholder`);
    for (const needle of ['<link rel="canonical"', 'property="og:image"', 'name="description"', "<title>"]) {
      if (!html.includes(needle)) problems.push(`${page}: missing ${needle}`);
    }
    for (const [, ref] of html.matchAll(/(?:href|src)="([^"#?]+)[^"]*"/g)) {
      if (/^(https?:|mailto:|data:)/.test(ref)) continue;
      const target = normalize(join(dir, dirname(page), ref));
      if (!existsSync(target)) problems.push(`${page}: ${ref} does not exist in the built site`);
    }
    const ids = new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
    for (const [, anchor] of html.matchAll(/href="#([^"]+)"/g)) {
      if (!ids.has(anchor)) problems.push(`${page}: #${anchor} points to no element`);
    }
  }
  return problems;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const dir = process.argv[2] ?? "_site";
  const problems = checkSite(dir);
  for (const p of problems) console.error(`::error::${p}`);
  if (problems.length) process.exit(1);
  console.log(`${dir}: links, images and metadata are complete.`);
}
