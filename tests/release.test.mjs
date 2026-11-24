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
