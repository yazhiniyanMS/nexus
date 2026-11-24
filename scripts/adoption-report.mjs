#!/usr/bin/env node
// Adoption report from public GitHub data and the repository's own traffic statistics.
// Uses the signed-in GitHub CLI (`gh`); collects nothing from users and adds no telemetry.
//
//   node scripts/adoption-report.mjs [--repo owner/name] [--save <folder>] [--json]
//
// --save writes a dated JSON snapshot to <folder> (keep it outside the repository; GitHub only
// keeps traffic for 14 days, so snapshots are how you see a trend) and compares with the latest
// earlier snapshot there.

import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const PLATFORM_OF = [
  [/\.dmg$/i, "macOS"],
  [/\.(exe|msi)$/i, "Windows"],
  [/\.(deb|rpm|AppImage)$/i, "Linux"],
];

/** Which system an installer is for, or null for checksums, manifests and the like. */
export function platformOf(assetName) {
  for (const [re, name] of PLATFORM_OF) if (re.test(assetName)) return name;
  return null;
}

/** Turns raw GitHub API responses into the numbers the report shows. Pure, so it can be tested. */
export function summarize({ repo, releases, issues, contributors, views, clones, referrers, owner, now }) {
  const since = new Date(new Date(now).getTime() - 30 * 86400_000);
  const installerDownloads = { macOS: 0, Windows: 0, Linux: 0 };
  const perRelease = releases.map((r) => {
    let total = 0;
    for (const a of r.assets ?? []) {
      const p = platformOf(a.name);
      if (!p) continue;
      installerDownloads[p] += a.download_count;
      total += a.download_count;
    }
    return { tag: r.tag_name, prerelease: r.prerelease, installerDownloads: total };
  });

  const onlyIssues = issues.filter((i) => !i.pull_request);
  const prs = issues.filter((i) => i.pull_request);
  const fromOthers = (i) => i.user?.login && i.user.login.toLowerCase() !== owner.toLowerCase() && i.user.type !== "Bot";
  const labelled = (name) => onlyIssues.filter((i) => (i.labels ?? []).some((l) => (l.name ?? l) === name));
  const recent = (i) => new Date(i.created_at) >= since;

  return {
    repo,
    at: now,
    stars: null, // filled by the caller from the repository record
    issues: {
      open: onlyIssues.filter((i) => i.state === "open").length,
      total: onlyIssues.length,
      fromOthers: onlyIssues.filter(fromOthers).length,
      last30Days: onlyIssues.filter(recent).length,
      installationProblems: labelled("installation").length,
      platformReports: labelled("platform-report").length,
      ux: labelled("ux").length,
      bugs: labelled("bug").length,
    },
    pullRequests: {
      total: prs.length,
      fromOthers: prs.filter(fromOthers).length,
      mergedFromOthers: prs.filter((p) => fromOthers(p) && p.pull_request?.merged_at).length,
    },
    contributorsOtherThanOwner: contributors.filter((c) => c.login.toLowerCase() !== owner.toLowerCase() && c.type !== "Bot").length,
    installerDownloads,
    perRelease,
    traffic14Days: {
      views: views?.count ?? null,
      uniqueVisitors: views?.uniques ?? null,
      clones: clones?.count ?? null,
      uniqueCloners: clones?.uniques ?? null,
      referrers: (referrers ?? []).map((r) => ({ from: r.referrer, views: r.count, unique: r.uniques })),
    },
  };
}

function delta(now, before) {
  if (before == null || now == null) return "";
  const d = now - before;
  return d === 0 ? " (±0)" : ` (${d > 0 ? "+" : ""}${d})`;
}

/** Markdown for the terminal or a private notes file. */
export function toMarkdown(s, prev) {
  const p = prev ?? {};
  const dl = s.installerDownloads;
  const pd = p.installerDownloads ?? {};
  const lines = [
    `# Adoption report — ${s.repo}`,
    "",
    `${s.at.slice(0, 10)}${prev ? `, compared with ${prev.at.slice(0, 10)}` : ""}. Public GitHub data and repository traffic only; no user is tracked.`,
    "",
    "| Measure | Value |",
    "| --- | --- |",
    `| Stars | ${s.stars}${delta(s.stars, p.stars)} |`,
    `| Visitors to the repository, last 14 days (unique) | ${s.traffic14Days.uniqueVisitors ?? "n/a"} |`,
    `| Clones, last 14 days (unique) | ${s.traffic14Days.uniqueCloners ?? "n/a"} |`,
    `| Installer downloads, all releases: macOS / Windows / Linux | ${dl.macOS}${delta(dl.macOS, pd.macOS)} / ${dl.Windows}${delta(dl.Windows, pd.Windows)} / ${dl.Linux}${delta(dl.Linux, pd.Linux)} |`,
    `| Issues opened by people other than the maintainer | ${s.issues.fromOthers}${delta(s.issues.fromOthers, p.issues?.fromOthers)} |`,
    `| Platform reports / installation problems | ${s.issues.platformReports} / ${s.issues.installationProblems} |`,
    `| Pull requests from others (merged) | ${s.pullRequests.fromOthers} (${s.pullRequests.mergedFromOthers}) |`,
    `| Contributors other than the maintainer | ${s.contributorsOtherThanOwner}${delta(s.contributorsOtherThanOwner, p.contributorsOtherThanOwner)} |`,
    "",
  ];
  if (s.traffic14Days.referrers.length) {
    lines.push("Where repository visitors came from (14 days):", "");
    for (const r of s.traffic14Days.referrers) lines.push(`- ${r.from}: ${r.views} views, ${r.unique} unique`);
    lines.push("");
  }
  lines.push(
    "Read with care: a download is a click on a file, not a user (it includes retries, CI-like fetches and people who never opened the app). Unique visitors are counted by GitHub over the 14 days. Clones include this repository's own CI runs, so they overstate people. Real use shows up as platform reports, issues and returning contributors.",
  );
  return lines.join("\n");
}

function gh(path, { paginate = false } = {}) {
  const args = ["api", path, ...(paginate ? ["--paginate", "--slurp"] : [])];
  try {
    const out = execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 << 20 });
    const data = JSON.parse(out);
    return paginate ? data.flat() : data;
  } catch {
    return null; // traffic needs push access; the rest of the report still works without it
  }
}

function main(argv) {
  const opt = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const repo = opt("--repo") ?? "yazhiniyanMS/nexus";
  const saveDir = opt("--save");
  const record = gh(`repos/${repo}`);
  if (!record) {
    console.error(`Could not read ${repo}. Is the GitHub CLI installed and signed in (gh auth status)?`);
    process.exit(1);
  }
  const summary = summarize({
    repo,
    owner: record.owner.login,
    now: new Date().toISOString(),
    releases: gh(`repos/${repo}/releases?per_page=100`, { paginate: true }) ?? [],
    issues: gh(`repos/${repo}/issues?state=all&per_page=100`, { paginate: true }) ?? [],
    contributors: gh(`repos/${repo}/contributors?per_page=100`, { paginate: true }) ?? [],
    views: gh(`repos/${repo}/traffic/views`),
    clones: gh(`repos/${repo}/traffic/clones`),
    referrers: gh(`repos/${repo}/traffic/popular/referrers`),
  });
  summary.stars = record.stargazers_count;
  summary.forks = record.forks_count;

  let prev;
  if (saveDir) {
    mkdirSync(saveDir, { recursive: true });
    const earlier = readdirSync(saveDir).filter((f) => /^adoption-\d{4}-\d{2}-\d{2}\.json$/.test(f)).sort();
    const today = `adoption-${summary.at.slice(0, 10)}.json`;
    const last = earlier.filter((f) => f !== today).pop();
    if (last) prev = JSON.parse(readFileSync(join(saveDir, last), "utf8"));
    writeFileSync(join(saveDir, today), JSON.stringify(summary, null, 2));
  }
  console.log(argv.includes("--json") ? JSON.stringify(summary, null, 2) : toMarkdown(summary, prev));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
