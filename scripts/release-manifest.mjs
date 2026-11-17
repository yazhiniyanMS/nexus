// Writes SHA256SUMS.txt and release-manifest.json for a folder of installers.
// Usage: node scripts/release-manifest.mjs <dir> --version 0.1.0 --commit <sha> [--signed macos,windows]
// Signing is recorded only when the workflow says a platform was signed; nothing is assumed.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const KINDS = [
  [/\.dmg$/i, "macos", "dmg"],
  [/\.app\.tar\.gz$/i, "macos", "app"],
  [/-setup\.exe$/i, "windows", "nsis"],
  [/\.msi$/i, "windows", "msi"],
  [/\.AppImage$/i, "linux", "appimage"],
  [/\.deb$/i, "linux", "deb"],
  [/\.rpm$/i, "linux", "rpm"],
];

/** Platform, package kind and CPU architecture from an installer's file name; null if unknown. */
export function classify(name) {
  const hit = KINDS.find(([re]) => re.test(name));
  if (!hit) return null;
  const arch = /aarch64|arm64/i.test(name) ? "arm64" : /x86_64|x64|amd64/i.test(name) ? "x64" : /i686|x86(?!_)/i.test(name) ? "x86" : "unknown";
  return { platform: hit[1], kind: hit[2], arch };
}

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

export function buildManifest(dir, { version, commit, signed = [] }) {
  const artifacts = readdirSync(dir)
    .filter((f) => statSync(join(dir, f)).isFile())
    .map((file) => ({ file, info: classify(file) }))
    .filter((a) => a.info)
    .sort((a, b) => a.file.localeCompare(b.file))
    .map(({ file, info }) => ({
      file,
      ...info,
      size: statSync(join(dir, file)).size,
      sha256: sha256(readFileSync(join(dir, file))),
      signed: signed.includes(info.platform),
    }));
  return { name: "NEXUS", version, commit: commit ?? null, generated_at: new Date().toISOString(), artifacts };
}

export const sumsFile = (manifest) => manifest.artifacts.map((a) => `${a.sha256}  ${a.file}\n`).join("");

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [dir, ...rest] = process.argv.slice(2);
  const arg = (name) => (rest.includes(name) ? rest[rest.indexOf(name) + 1] : undefined);
  if (!dir || !arg("--version")) {
    console.error("usage: release-manifest.mjs <dir> --version <v> [--commit <sha>] [--signed macos,windows]");
    process.exit(2);
  }
  const manifest = buildManifest(dir, { version: arg("--version"), commit: arg("--commit"), signed: (arg("--signed") ?? "").split(",").filter(Boolean) });
  if (manifest.artifacts.length === 0) {
    console.error(`No installers found in ${dir}`);
    process.exit(1);
  }
  writeFileSync(join(dir, "SHA256SUMS.txt"), sumsFile(manifest));
  writeFileSync(join(dir, "release-manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  for (const a of manifest.artifacts) console.log(`${a.sha256}  ${a.file}  (${a.platform}/${a.arch}/${a.kind}, ${a.signed ? "signed" : "unsigned"})`);
}
