//! The codebase report: one self-contained HTML file that explains a project's shape to someone
//! who has never seen it, built from NEXUS's own analysis and code index. No AI, no network.
//!
//! It contains file paths, module names, dependency names and counts, never source text:
//! findings whose message quotes a line of code (to-do and fix-me markers) are reduced to their location,
//! so the file can be shared without sharing the code. It has no scripts and a CSP that forbids
//! loading anything, so opening it does nothing but display it.

use super::graph::Graph;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A dependency manifest: (path, ecosystem, [(package, version)]).
pub type Manifest = (String, String, Vec<(String, String)>);
/// A finding: (severity, kind, message, file, line).
pub type Finding = (String, String, String, Option<String>, Option<u32>);

/// Everything the report shows, gathered by the caller.
pub struct ReportInput<'a> {
    pub name: &'a str,
    pub generated_at: &'a str,
    pub version: &'a str,
    pub summary: &'a str,
    pub frameworks: Vec<String>,
    /// (language, files, lines) from the code index.
    pub languages: Vec<(String, u64, u64)>,
    pub files: u64,
    pub lines: u64,
    pub symbols: u64,
    pub modules: &'a Graph,
    /// (path, files importing it), most imported first.
    pub hotspots: Vec<(String, u32)>,
    pub entry_points: Vec<String>,
    /// (package, files importing it).
    pub externals: Vec<(String, u32)>,
    pub manifests: Vec<Manifest>,
    pub findings: Vec<Finding>,
    pub git: Option<GitFacts>,
}

#[derive(Default)]
pub struct GitFacts {
    pub branch: Option<String>,
    pub commits: u64,
    pub contributors: u64,
    pub first_commit: Option<String>,
    pub last_commit: Option<String>,
    pub uncommitted: u64,
}

/// Escape text for HTML element content and attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

fn num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Kinds whose message quotes source text; the report shows only where they are.
const QUOTES_SOURCE: &[&str] = &["marker"];

const MAX_GRAPH_MODULES: usize = 36;

const BOX_W: f64 = 176.0;
const BOX_H: f64 = 44.0;

/// Strongly connected components (Kosaraju): modules in one dependency cycle end up together.
fn components(ids: &[&str], edges: &[(usize, usize)]) -> Vec<usize> {
    let n = ids.len();
    let (mut fwd, mut back) = (vec![vec![]; n], vec![vec![]; n]);
    for &(a, b) in edges {
        fwd[a].push(b);
        back[b].push(a);
    }
    fn visit(v: usize, adj: &[Vec<usize>], seen: &mut [bool], out: &mut Vec<usize>) {
        seen[v] = true;
        for &w in &adj[v] {
            if !seen[w] {
                visit(w, adj, seen, out);
            }
        }
        out.push(v);
    }
    let (mut seen, mut order) = (vec![false; n], vec![]);
    for v in 0..n {
        if !seen[v] {
            visit(v, &fwd, &mut seen, &mut order);
        }
    }
    let (mut comp, mut next) = (vec![usize::MAX; n], 0);
    for &v in order.iter().rev() {
        if comp[v] == usize::MAX {
            let (mut seen2, mut members) = (vec![false; n], vec![]);
            for (u, c) in comp.iter().enumerate() {
                seen2[u] = *c != usize::MAX;
            }
            visit(v, &back, &mut seen2, &mut members);
            for m in members {
                comp[m] = next;
            }
            next += 1;
        }
    }
    comp
}

/// Lay out module nodes in layers: modules nothing depends on at the bottom, importers above
/// what they import. Modules in the same cycle share a layer, so loops do not stretch the
/// picture. Returns (id, x, y) and the canvas size.
fn layout(g: &Graph) -> (Vec<(String, f64, f64)>, f64, f64) {
    let mut modules: Vec<&super::graph::Node> = g.nodes.iter().filter(|n| n.kind == "module").collect();
    modules.sort_by_key(|n| std::cmp::Reverse(n.lines));
    modules.truncate(MAX_GRAPH_MODULES);
    let ids: Vec<&str> = modules.iter().map(|n| n.id.as_str()).collect();
    let index = |id: &str| ids.iter().position(|x| *x == id);
    let edges: Vec<(usize, usize)> =
        g.edges.iter().filter_map(|e| Some((index(&e.from)?, index(&e.to)?))).filter(|(a, b)| a != b).collect();
    let comp = components(&ids, &edges);
    let comps = comp.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    // Longest path in the component graph (it has no cycles), dependencies first.
    let mut level = vec![0usize; comps];
    for _ in 0..comps {
        let mut changed = false;
        for &(a, b) in &edges {
            let (ca, cb) = (comp[a], comp[b]);
            if ca != cb && level[ca] < level[cb] + 1 {
                level[ca] = level[cb] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut used: Vec<usize> = level.clone();
    used.sort_unstable();
    used.dedup();
    let compact = |l: usize| used.iter().position(|u| *u == l).unwrap_or(0);
    let top = used.len().saturating_sub(1);
    let mut rows: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        rows.entry(compact(level[comp[i]])).or_default().push(id);
    }
    let (gap_x, gap_y, per_row) = (22.0, 64.0, 5usize);
    // Wide layers wrap onto extra rows so the picture stays readable.
    let mut placed: Vec<(String, usize, usize)> = vec![];
    let mut row_index = 0usize;
    let mut row_widths: Vec<usize> = vec![];
    for lvl in (0..=top).rev() {
        let Some(row) = rows.get(&lvl) else { continue };
        for chunk in row.chunks(per_row) {
            for (i, id) in chunk.iter().enumerate() {
                placed.push((id.to_string(), row_index, i));
            }
            row_widths.push(chunk.len());
            row_index += 1;
        }
    }
    let widest = row_widths.iter().copied().max().unwrap_or(1) as f64;
    let width = (widest * (BOX_W + gap_x) + gap_x).max(400.0);
    let height = row_index as f64 * (BOX_H + gap_y) - gap_y + 40.0;
    let out = placed
        .into_iter()
        .map(|(id, r, i)| {
            let row_width = row_widths[r] as f64 * (BOX_W + gap_x) - gap_x;
            (id, (width - row_width) / 2.0 + i as f64 * (BOX_W + gap_x), 20.0 + r as f64 * (BOX_H + gap_y))
        })
        .collect();
    (out, width, height)
}

fn graph_svg(g: &Graph) -> String {
    let (pos, width, height) = layout(g);
    if pos.is_empty() {
        return String::new();
    }
    let at: BTreeMap<&str, (f64, f64)> = pos.iter().map(|(id, x, y)| (id.as_str(), (*x, *y))).collect();
    let node = |id: &str| g.nodes.iter().find(|n| n.id == id);
    let (w, h) = (BOX_W, BOX_H);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg class="graph" viewBox="0 0 {width:.0} {height:.0}" role="img" aria-label="Module dependency graph" xmlns="http://www.w3.org/2000/svg"><defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" class="arrow"/></marker></defs>"#
    );
    for e in &g.edges {
        let (Some(&(fx, fy)), Some(&(tx, ty))) = (at.get(e.from.as_str()), at.get(e.to.as_str())) else { continue };
        let (x1, y1) = (fx + w / 2.0, if fy <= ty { fy + h } else { fy });
        let (x2, y2) = (tx + w / 2.0, if fy <= ty { ty } else { ty + h });
        let mid = (y1 + y2) / 2.0;
        let width = (1.0 + (e.weight as f64).ln_1p()).min(4.0);
        let _ = write!(
            s,
            r#"<path class="edge" d="M{x1:.0} {y1:.0} C{x1:.0} {mid:.0} {x2:.0} {mid:.0} {x2:.0} {y2:.0}" stroke-width="{width:.1}" marker-end="url(#a)"><title>{} → {} ({} import{})</title></path>"#,
            esc(&e.from),
            esc(&e.to),
            e.weight,
            if e.weight == 1 { "" } else { "s" }
        );
    }
    for (id, x, y) in &pos {
        let n = node(id);
        let cycle = n.is_some_and(|n| n.in_cycle);
        // Monospace at 12px fits about 21 characters in a box; keep the end of long paths.
        let label: String = if id.chars().count() > 21 {
            format!("…{}", id.chars().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<String>())
        } else {
            id.clone()
        };
        let _ = write!(
            s,
            r#"<g class="node{}"><title>{}</title><rect x="{x:.0}" y="{y:.0}" width="{w:.0}" height="{h:.0}" rx="7"/><text x="{:.0}" y="{:.0}" class="name">{}</text><text x="{:.0}" y="{:.0}" class="meta">{} file{} · {} lines</text></g>"#,
            if cycle { " cycle" } else { "" },
            esc(id),
            x + 10.0,
            y + 19.0,
            esc(&label),
            x + 10.0,
            y + 35.0,
            n.map(|n| n.files).unwrap_or(0),
            if n.map(|n| n.files).unwrap_or(0) == 1 { "" } else { "s" },
            num(n.map(|n| n.lines).unwrap_or(0)),
        );
    }
    s.push_str("</svg>");
    s
}

const STYLE: &str = r#"
:root{--bg:#fbfbfa;--panel:#fff;--ink:#17181a;--dim:#5b5f66;--faint:#8a8f98;--line:#e4e4e0;--accent:#0f7b5f;--warn:#a15c07;--danger:#b42318;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
@media (prefers-color-scheme:dark){:root{--bg:#0d0e10;--panel:#15171a;--ink:#ecedee;--dim:#a3a7ae;--faint:#6f747c;--line:#26292e;--accent:#4fd1a5;--warn:#f0b35b;--danger:#ff7b72}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,sans-serif}
main{max-width:1040px;margin:0 auto;padding:40px 24px 64px}h1{font-size:28px;margin:0 0 4px;letter-spacing:-.01em}h2{font-size:17px;margin:40px 0 12px;padding-top:20px;border-top:1px solid var(--line)}
.sub{color:var(--dim);margin:0}.meta-line{color:var(--faint);font-size:12px;margin-top:6px}code,.mono{font-family:var(--mono);font-size:12.5px}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:10px;margin-top:24px}.stat{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px 14px}.stat b{display:block;font-size:20px;font-variant-numeric:tabular-nums}.stat span{color:var(--dim);font-size:12px}
.bar{display:flex;height:10px;border-radius:5px;overflow:hidden;background:var(--line);margin:8px 0 10px}.bar i{display:block}
.legend{display:flex;flex-wrap:wrap;gap:6px 16px;font-size:12.5px;color:var(--dim)}.legend span::before{content:"";display:inline-block;width:9px;height:9px;border-radius:2px;margin-right:6px;background:var(--c)}
table{width:100%;border-collapse:collapse;font-size:13px}th{text-align:left;color:var(--faint);font-weight:500;font-size:12px;border-bottom:1px solid var(--line);padding:6px 8px 6px 0}td{border-bottom:1px solid var(--line);padding:6px 8px 6px 0;vertical-align:top}td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}
.graph{width:100%;height:auto;background:var(--panel);border:1px solid var(--line);border-radius:10px;margin-top:4px}.graph rect{fill:var(--bg);stroke:var(--line);stroke-width:1.2}.graph .cycle rect{stroke:var(--danger);stroke-width:2}.graph .name{font:600 12px var(--mono);fill:var(--ink)}.graph .meta{font-size:10.5px;fill:var(--faint)}.graph .edge{fill:none;stroke:var(--faint);opacity:.55}.graph .arrow{fill:var(--faint)}
.pill{display:inline-block;border-radius:999px;padding:0 8px;font-size:11.5px;border:1px solid var(--line);color:var(--dim)}.high{color:var(--danger);border-color:currentColor}.medium{color:var(--warn);border-color:currentColor}
.cols{display:grid;grid-template-columns:1fr 1fr;gap:28px}@media(max-width:760px){.cols{grid-template-columns:1fr}}
.note{background:var(--panel);border:1px solid var(--line);border-left:3px solid var(--accent);border-radius:6px;padding:10px 14px;color:var(--dim);font-size:13px;margin-top:20px}
footer{margin-top:48px;color:var(--faint);font-size:12px}
"#;

const PALETTE: &[&str] = &["#0f7b5f", "#3b6fd8", "#c2410c", "#7c3aed", "#b45309", "#0e7490", "#be185d", "#4d7c0f"];

pub fn render(d: &ReportInput) -> String {
    let mut o = String::new();
    let _ = write!(
        o,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:"><meta name="generator" content="NEXUS {v}"><title>{n} — codebase report</title><style>{STYLE}</style></head><body><main>"#,
        v = esc(d.version),
        n = esc(d.name)
    );
    let _ =
        write!(o, "<h1>{}</h1><p class=\"sub\">{}</p>", esc(d.name), esc(if d.summary.is_empty() { "Codebase report" } else { d.summary }));
    if !d.frameworks.is_empty() {
        let _ = write!(o, "<p class=\"meta-line\">Stack: {}</p>", esc(&d.frameworks.join(", ")));
    }

    let high = d.findings.iter().filter(|f| f.0 == "high").count() as u64;
    let modules = d.modules.nodes.iter().filter(|n| n.kind == "module").count() as u64;
    let deps: usize = d.manifests.iter().map(|m| m.2.len()).sum();
    o.push_str("<div class=\"stats\">");
    for (value, label) in [
        (num(d.files), "source files indexed"),
        (num(d.lines), "lines of code"),
        (num(d.symbols), "symbols"),
        (num(modules), "modules"),
        (num(d.modules.cycles.len() as u64), "dependency cycles"),
        (num(deps as u64), "declared dependencies"),
        (num(high), "high-severity findings"),
    ] {
        let _ = write!(o, "<div class=\"stat\"><b>{value}</b><span>{label}</span></div>");
    }
    o.push_str("</div>");

    if !d.languages.is_empty() {
        let total: u64 = d.languages.iter().map(|l| l.2).sum::<u64>().max(1);
        o.push_str("<h2>Languages</h2><div class=\"bar\">");
        for (i, (_, _, lines)) in d.languages.iter().enumerate() {
            let _ =
                write!(o, "<i style=\"width:{:.2}%;background:{}\"></i>", *lines as f64 * 100.0 / total as f64, PALETTE[i % PALETTE.len()]);
        }
        o.push_str("</div><div class=\"legend\">");
        for (i, (name, files, lines)) in d.languages.iter().enumerate() {
            let _ = write!(
                o,
                "<span style=\"--c:{}\">{} · {} files · {:.0}%</span>",
                PALETTE[i % PALETTE.len()],
                esc(name),
                num(*files),
                *lines as f64 * 100.0 / total as f64
            );
        }
        o.push_str("</div>");
    }

    o.push_str("<h2>Architecture</h2>");
    let svg = graph_svg(d.modules);
    if svg.is_empty() {
        o.push_str(
            "<p class=\"sub\">No modules were found in the code index (indexed languages: Rust, TypeScript, JavaScript, Python, Go).</p>",
        );
    } else {
        o.push_str("<p class=\"sub\">Modules (folders) and the imports between them. A module sits above the modules it uses; thicker lines mean more imports; a red outline marks a dependency cycle.</p>");
        o.push_str(&svg);
        if !d.modules.cycles.is_empty() {
            o.push_str("<p><b>Cycles</b> — modules that depend on each other in a loop, which makes them hard to change or test separately:</p><ul>");
            for c in &d.modules.cycles {
                let _ = write!(o, "<li class=\"mono\">{}</li>", esc(&c.join(" ↔ ")));
            }
            o.push_str("</ul>");
        }
        let mut mods: Vec<_> = d.modules.nodes.iter().filter(|n| n.kind == "module").collect();
        mods.sort_by_key(|n| std::cmp::Reverse(n.lines));
        o.push_str("<table><tr><th>Module</th><th class=\"n\">Files</th><th class=\"n\">Lines</th><th class=\"n\">Uses</th><th class=\"n\">Used by</th></tr>");
        for n in mods.iter().take(40) {
            let _ = write!(
                o,
                "<tr><td class=\"mono\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
                esc(&n.id),
                n.files,
                num(n.lines),
                n.fan_out,
                n.fan_in
            );
        }
        o.push_str("</table>");
    }

    o.push_str("<div class=\"cols\"><div>");
    o.push_str("<h2>Where to start reading</h2>");
    if d.entry_points.is_empty() {
        o.push_str("<p class=\"sub\">No conventional entry points (main, index, app, server…) were found.</p>");
    } else {
        o.push_str("<p class=\"sub\">Entry points by convention:</p><ul>");
        for e in &d.entry_points {
            let _ = write!(o, "<li class=\"mono\">{}</li>", esc(e));
        }
        o.push_str("</ul>");
    }
    if !d.hotspots.is_empty() {
        o.push_str("<p class=\"sub\" style=\"margin-top:14px\">Most-imported files — changes here reach the most code:</p><table><tr><th>File</th><th class=\"n\">Imported by</th></tr>");
        for (path, n) in &d.hotspots {
            let _ = write!(o, "<tr><td class=\"mono\">{}</td><td class=\"n\">{n}</td></tr>", esc(path));
        }
        o.push_str("</table>");
    }
    o.push_str("</div><div>");
    o.push_str("<h2>External packages in use</h2>");
    if d.externals.is_empty() {
        o.push_str("<p class=\"sub\">No external imports were found in indexed files.</p>");
    } else {
        o.push_str("<table><tr><th>Package</th><th class=\"n\">Files importing it</th></tr>");
        for (pkg, n) in d.externals.iter().take(20) {
            let _ = write!(o, "<tr><td class=\"mono\">{}</td><td class=\"n\">{n}</td></tr>", esc(pkg));
        }
        o.push_str("</table>");
    }
    o.push_str("</div></div>");

    if !d.manifests.is_empty() {
        o.push_str("<h2>Declared dependencies</h2><div class=\"cols\">");
        for (manifest, ecosystem, deps) in &d.manifests {
            let _ = write!(
                o,
                "<div><p><b class=\"mono\">{}</b> <span class=\"pill\">{}</span> {} packages</p><table>",
                esc(manifest),
                esc(ecosystem),
                deps.len()
            );
            for (name, version) in deps.iter().take(30) {
                let _ = write!(
                    o,
                    "<tr><td class=\"mono\">{}</td><td class=\"mono\" style=\"color:var(--faint)\">{}</td></tr>",
                    esc(name),
                    esc(version)
                );
            }
            if deps.len() > 30 {
                let _ = write!(o, "<tr><td colspan=\"2\" style=\"color:var(--faint)\">and {} more</td></tr>", deps.len() - 30);
            }
            o.push_str("</table></div>");
        }
        o.push_str("</div>");
    }

    o.push_str("<h2>Findings</h2>");
    if d.findings.is_empty() {
        o.push_str("<p class=\"sub\">No findings.</p>");
    } else {
        let mut by: BTreeMap<&str, usize> = BTreeMap::new();
        for f in &d.findings {
            *by.entry(f.0.as_str()).or_default() += 1;
        }
        let order = ["high", "medium", "low", "info"];
        let summary: Vec<String> = order.iter().filter_map(|s| by.get(s).map(|n| format!("{n} {s}"))).collect();
        let _ = write!(o, "<p class=\"sub\">{} — from NEXUS's analysis: possible secrets, committed environment files, very large files, missing tests, README, licence or CI, and code markers.</p>", esc(&summary.join(" · ")));
        o.push_str("<table><tr><th>Severity</th><th>Finding</th><th>Where</th></tr>");
        let mut sorted: Vec<_> = d.findings.iter().collect();
        sorted.sort_by_key(|f| order.iter().position(|s| *s == f.0).unwrap_or(9));
        for (sev, kind, message, file, line) in sorted.iter().take(60) {
            let text = if QUOTES_SOURCE.contains(&kind.as_str()) {
                "Code marker (to-do, fix-me or hack note)".to_string()
            } else {
                message.clone()
            };
            let place = match (file, line) {
                (Some(f), Some(l)) => format!("{f}:{l}"),
                (Some(f), None) => f.clone(),
                _ => String::new(),
            };
            let _ = write!(
                o,
                "<tr><td><span class=\"pill {}\">{}</span></td><td>{}</td><td class=\"mono\">{}</td></tr>",
                esc(sev),
                esc(sev),
                esc(&text),
                esc(&place)
            );
        }
        o.push_str("</table>");
    }

    if let Some(g) = &d.git {
        o.push_str("<h2>Repository</h2><table>");
        let rows = [
            ("Branch", g.branch.clone().unwrap_or_else(|| "detached or none".into())),
            ("Commits", num(g.commits)),
            ("Contributors", num(g.contributors)),
            ("First commit", g.first_commit.clone().unwrap_or_default()),
            ("Last commit", g.last_commit.clone().unwrap_or_default()),
            ("Uncommitted changes when generated", num(g.uncommitted)),
        ];
        for (k, v) in rows {
            let _ = write!(o, "<tr><td style=\"color:var(--dim);width:40%\">{k}</td><td>{}</td></tr>", esc(&v));
        }
        o.push_str("</table>");
    }

    let _ = write!(
        o,
        "<div class=\"note\">This report lists file paths, module and package names and counts. It contains no source code. It was generated locally by NEXUS {} from its own analysis and code index, without AI and without network access.</div><footer>Generated {} · NEXUS {} · open-source developer workspace</footer></main></body></html>",
        esc(d.version),
        esc(d.generated_at),
        esc(d.version)
    );
    o
}
