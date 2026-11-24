import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { check, versions } from "../scripts/check-version.mjs";
import { buildManifest, classify, sha256, sumsFile } from "../scripts/release-manifest.mjs";

test("installer names map to platform, kind and architecture", () => {
  assert.deepEqual(classify("NEXUS_0.1.0_aarch64.dmg"), { platform: "macos", kind: "dmg", arch: "arm64" });
  assert.deepEqual(classify("NEXUS_0.1.0_x64.dmg"), { platform: "macos", kind: "dmg", arch: "x64" });
  assert.deepEqual(classify("NEXUS_0.1.0_x64-setup.exe"), { platform: "windows", kind: "nsis", arch: "x64" });
  assert.deepEqual(classify("NEXUS_0.1.0_x64_en-US.msi"), { platform: "windows", kind: "msi", arch: "x64" });
  assert.deepEqual(classify("NEXUS_0.1.0_amd64.AppImage"), { platform: "linux", kind: "appimage", arch: "x64" });
  assert.deepEqual(classify("NEXUS_0.1.0_amd64.deb"), { platform: "linux", kind: "deb", arch: "x64" });
  assert.deepEqual(classify("NEXUS-0.1.0-1.x86_64.rpm"), { platform: "linux", kind: "rpm", arch: "x64" });
  assert.equal(classify("notes.txt"), null);
});

test("the manifest lists real sizes and checksums and never assumes signing", () => {
  const dir = mkdtempSync(join(tmpdir(), "nexus-manifest-"));
  writeFileSync(join(dir, "NEXUS_0.1.0_amd64.deb"), "deb-bytes");
  writeFileSync(join(dir, "NEXUS_0.1.0_aarch64.dmg"), "dmg-bytes");
  writeFileSync(join(dir, "README.txt"), "ignored");
  const m = buildManifest(dir, { version: "0.1.0", commit: "abc123", signed: ["macos"] });
  assert.deepEqual(m.artifacts.map((a) => a.file), ["NEXUS_0.1.0_aarch64.dmg", "NEXUS_0.1.0_amd64.deb"]);
  assert.equal(m.artifacts[1].sha256, sha256("deb-bytes"));
  assert.equal(m.artifacts[1].size, 9);
  assert.deepEqual(m.artifacts.map((a) => a.signed), [true, false]);
  assert.equal(sumsFile(m), `${sha256("dmg-bytes")}  NEXUS_0.1.0_aarch64.dmg\n${sha256("deb-bytes")}  NEXUS_0.1.0_amd64.deb\n`);
  assert.equal(buildManifest(dir, { version: "0.1.0" }).artifacts.every((a) => a.signed === false), true);
  rmSync(dir, { recursive: true });
});

test("version files agree, and tags must match them", () => {
  const found = versions();
  assert.deepEqual(check(found), [], JSON.stringify(found));
  const v = Object.values(found)[0];
  assert.deepEqual(check(found, `v${v}`), []);
  assert.equal(check(found, "v9.9.9").length, 1);
  assert.equal(check({ a: "1.0.0", b: "1.0.1" }).length, 1);
  assert.match(check({ a: "1.0.0-beta.1" })[0], /numeric/);
  assert.equal(readFileSync("rust-toolchain.toml", "utf8").includes("channel"), true);
});

test("the website fills in the repository, site address and version and leaves no placeholder", async () => {
  const { render, pagesUrl, sitemap, robots, ASSETS } = await import("../scripts/build-site.mjs");
  const html = render(readFileSync("website/index.html", "utf8"), { repo: "Example-Owner/nexus", version: "0.1.0" });
  assert.match(html, /https:\/\/github\.com\/Example-Owner\/nexus\/releases/);
  assert.match(html, /<link rel="canonical" href="https:\/\/example-owner\.github\.io\/nexus\/">/);
  assert.match(html, /og:image" content="https:\/\/example-owner\.github\.io\/nexus\/assets\/social-preview\.png"/);
  assert.equal(html.includes("{{"), false);
  assert.throws(() => render("{{REPO_URL}}", { repo: "not a repo", version: "1" }));
  assert.throws(() => render("x", { repo: "a/b", version: "1", site: "http://insecure.example/" }));
  assert.equal(pagesUrl("A/b"), "https://a.github.io/b/");
  assert.match(sitemap("https://a.github.io/b/"), /<loc>https:\/\/a\.github\.io\/b\/<\/loc>/);
  assert.match(robots("https://a.github.io/b/"), /Sitemap: https:\/\/a\.github\.io\/b\/sitemap\.xml/);
  // The structured data is valid JSON.
  const ld = html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)[1];
  assert.equal(JSON.parse(ld).name, "NEXUS");
  // Every local file the page uses exists: in website/, or in docs/assets (copied in at build time).
  const local = [...html.matchAll(/(?:src|href)="(assets\/[^"]+)"/g)].map((m) => m[1]);
  assert.ok(local.length >= 8, "the page shows the screenshots");
  for (const src of local) {
    const fromDocs = ASSETS.some((a) => src.startsWith(`assets/${a}`));
    assert.doesNotThrow(() => readFileSync(fromDocs ? `docs/${src}` : `website/${src}`), src);
  }
});

test("the site checker catches broken links, missing metadata and dangling anchors", async () => {
  const { checkSite } = await import("../scripts/check-site.mjs");
  const dir = mkdtempSync(join(tmpdir(), "nexus-site-"));
  const head = '<title>t</title><meta name="description" content="d"><link rel="canonical" href="https://x/"><meta property="og:image" content="https://x/i.png">';
  writeFileSync(join(dir, "styles.css"), "");
  writeFileSync(join(dir, "robots.txt"), "");
  writeFileSync(join(dir, "sitemap.xml"), "");
  writeFileSync(join(dir, "index.html"), `${head}<a href="#here">ok</a><p id="here"></p><a href="styles.css">ok</a><a href="https://example.com">ok</a>`);
  assert.deepEqual(checkSite(dir), []);
  writeFileSync(join(dir, "index.html"), `<title>t</title><img src="assets/missing.png"><a href="#nowhere">x</a>{{VERSION}}`);
  const problems = checkSite(dir).join("\n");
  assert.match(problems, /assets\/missing\.png does not exist/);
  assert.match(problems, /#nowhere points to no element/);
  assert.match(problems, /unfilled placeholder/);
  assert.match(problems, /missing <link rel="canonical"/);
  rmSync(dir, { recursive: true });
});

test("the adoption report counts installers by system and never counts checksums or the maintainer", async () => {
  const { summarize, toMarkdown, platformOf } = await import("../scripts/adoption-report.mjs");
  assert.equal(platformOf("SHA256SUMS.txt"), null);
  assert.equal(platformOf("NEXUS_0.2.0_amd64.AppImage"), "Linux");
  const s = summarize({
    repo: "o/r",
    owner: "Owner",
    now: "2026-12-01T00:00:00Z",
    releases: [{ tag_name: "v1", prerelease: true, assets: [
      { name: "NEXUS_1_aarch64.dmg", download_count: 3 },
      { name: "NEXUS_1_x64-setup.exe", download_count: 2 },
      { name: "NEXUS_1_x64_en-US.msi", download_count: 1 },
      { name: "NEXUS_1_amd64.deb", download_count: 4 },
      { name: "SHA256SUMS.txt", download_count: 9 },
      { name: "release-manifest.json", download_count: 9 },
    ] }],
    issues: [
      { state: "open", user: { login: "owner", type: "User" }, labels: [{ name: "bug" }], created_at: "2026-11-30T00:00:00Z" },
      { state: "open", user: { login: "someone", type: "User" }, labels: [{ name: "platform-report" }], created_at: "2026-11-29T00:00:00Z" },
      { state: "closed", user: { login: "dependabot[bot]", type: "Bot" }, labels: [], created_at: "2026-01-01T00:00:00Z", pull_request: { merged_at: "2026-01-02T00:00:00Z" } },
      { state: "closed", user: { login: "helper", type: "User" }, labels: [], created_at: "2026-11-20T00:00:00Z", pull_request: { merged_at: "2026-11-21T00:00:00Z" } },
    ],
    contributors: [{ login: "Owner", type: "User" }, { login: "helper", type: "User" }],
    views: { count: 10, uniques: 4 },
    clones: null,
    referrers: [],
  });
  assert.deepEqual(s.installerDownloads, { macOS: 3, Windows: 3, Linux: 4 });
  assert.equal(s.perRelease[0].installerDownloads, 10);
  assert.equal(s.issues.total, 2);
  assert.equal(s.issues.fromOthers, 1);
  assert.equal(s.issues.platformReports, 1);
  assert.equal(s.pullRequests.fromOthers, 1);
  assert.equal(s.pullRequests.mergedFromOthers, 1);
  assert.equal(s.contributorsOtherThanOwner, 1);
  s.stars = 5;
  const md = toMarkdown(s, { at: "2026-11-24T00:00:00Z", stars: 4, installerDownloads: { macOS: 1, Windows: 3, Linux: 4 } });
  assert.match(md, /\| Stars \| 5 \(\+1\) \|/);
  assert.match(md, /3 \(\+2\) \/ 3 \(±0\) \/ 4 \(±0\)/);
  assert.match(md, /not a user/);
});
