// Checks that package.json, Cargo.toml and tauri.conf.json agree on the version, and (when a tag
// is given) that the tag is "v" + that version. Usage: node scripts/check-version.mjs [tag]
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export function versions(root = ".") {
  const read = (p) => readFileSync(`${root}/${p}`, "utf8");
  return {
    "package.json": JSON.parse(read("package.json")).version,
    "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
    "src-tauri/Cargo.toml": /^version\s*=\s*"([^"]+)"/m.exec(read("src-tauri/Cargo.toml"))?.[1],
  };
}

/** Problems found, as messages; an empty list means everything agrees. */
export function check(found, tag) {
  const problems = [];
  const distinct = [...new Set(Object.values(found))];
  if (distinct.length !== 1 || !distinct[0]) {
    problems.push(`Versions differ: ${Object.entries(found).map(([f, v]) => `${f}=${v}`).join(", ")}`);
  }
  // MSI installers accept only numeric major.minor.patch.
  if (distinct[0] && !/^\d+\.\d+\.\d+$/.test(distinct[0])) problems.push(`Version ${distinct[0]} must be numeric major.minor.patch`);
  if (tag && tag !== `v${distinct[0]}`) problems.push(`Tag ${tag} does not match version ${distinct[0]} (expected v${distinct[0]})`);
  return problems;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const found = versions();
  const problems = check(found, process.argv[2]);
  if (problems.length) {
    console.error(problems.join("\n"));
    process.exit(1);
  }
  console.log(`Version ${Object.values(found)[0]} is consistent${process.argv[2] ? ` and matches ${process.argv[2]}` : ""}.`);
}
