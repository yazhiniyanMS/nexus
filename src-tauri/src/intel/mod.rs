//! Project Intelligence: a code index (files, symbols, imports) and everything derived from it —
//! symbol and reference search, dependency graphs, cross-file context, source-linked answers and
//! architecture documents. Everything except the optional AI answer works offline.

pub mod context;
pub mod docs;
pub mod extract;
pub mod graph;
pub mod report;
pub mod resolve;

use crate::error::{AppError, Result};
use crate::projects::{project_root, read_text, rel, walker};
use crate::{activity, ai, db, AppState};
use extract::{lang_of, Symbol};
use regex::Regex;
use resolve::{Resolver, Target};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_dialog::DialogExt;

const MAX_INDEX_FILES: usize = 20_000;
const MAX_GRAPH_NODES: usize = 160;
const MAX_REFERENCES: usize = 400;
pub const EVENT_PROGRESS: &str = "nexus://index-progress";

pub struct IndexData {
    /// (path, language, lines)
    pub files: Vec<(String, &'static str, u64)>,
    pub symbols: Vec<(String, Symbol)>,
    /// (from, target, external, line)
    pub imports: Vec<(String, String, bool, u32)>,
}

/// Walk a project and extract symbols and resolved imports from every supported source file.
pub fn build_index(root: &Path, mut progress: impl FnMut(usize, usize)) -> IndexData {
    let all: BTreeSet<String> =
        walker(root).flatten().filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false)).map(|e| rel(root, e.path())).collect();
    let go_module = std::fs::read_to_string(root.join("go.mod"))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("module ").map(|m| m.trim().to_string())));
    let resolver = Resolver { files: &all, go_module };
    let sources: Vec<&String> = all.iter().filter(|p| lang_of(p).is_some()).take(MAX_INDEX_FILES).collect();
    let mut data = IndexData { files: vec![], symbols: vec![], imports: vec![] };
    for (i, path) in sources.iter().enumerate() {
        if i % 50 == 0 {
            progress(i, sources.len());
        }
        let (lang, lang_name) = lang_of(path).unwrap();
        let Some(text) = read_text(&root.join(path.as_str())) else { continue };
        data.files.push(((*path).clone(), lang_name, text.lines().count() as u64));
        let (symbols, imports) = extract::extract(lang, &text);
        data.symbols.extend(symbols.into_iter().map(|s| ((*path).clone(), s)));
        let mut seen = HashSet::new();
        for imp in imports {
            let (target, external) = match resolver.resolve(lang, path, &imp.spec) {
                Target::Internal(t) => (t, false),
                Target::External(t) => (t, true),
                Target::Ignored => continue,
            };
            if target != **path && seen.insert((target.clone(), external)) {
                data.imports.push(((*path).clone(), target, external, imp.line));
            }
        }
    }
    progress(sources.len(), sources.len());
    data
}

/// Replace a project's index atomically.
pub fn store_index(conn: &mut Connection, project_id: &str, data: &IndexData) -> Result<()> {
    let tx = conn.transaction()?;
    for table in ["intel_files", "intel_symbols", "intel_imports"] {
        tx.execute(&format!("DELETE FROM {table} WHERE project_id = ?1"), [project_id])?;
    }
    {
        let mut f = tx.prepare("INSERT INTO intel_files (project_id, path, language, lines) VALUES (?1, ?2, ?3, ?4)")?;
        for (path, lang, lines) in &data.files {
            f.execute(params![project_id, path, lang, *lines as i64])?;
        }
        let mut s =
            tx.prepare("INSERT INTO intel_symbols (project_id, path, name, kind, line, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for (path, sym) in &data.symbols {
            s.execute(params![project_id, path, sym.name, sym.kind, sym.line, sym.signature])?;
        }
        let mut i = tx.prepare("INSERT INTO intel_imports (project_id, from_path, target, external, line) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for (from, target, external, line) in &data.imports {
            i.execute(params![project_id, from, target, *external as i64, line])?;
        }
    }
    tx.execute("UPDATE projects SET indexed_at = ?1 WHERE id = ?2", params![db::now(), project_id])?;
    tx.commit()?;
    Ok(())
}

#[derive(Serialize, Debug)]
pub struct IndexStatus {
    indexed_at: Option<String>,
    files: i64,
    symbols: i64,
    imports: i64,
}

pub fn status(conn: &Connection, project_id: &str) -> Result<IndexStatus> {
    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE project_id = ?1"), [project_id], |r| r.get(0))?)
    };
    Ok(IndexStatus {
        indexed_at: conn.query_row("SELECT indexed_at FROM projects WHERE id = ?1", [project_id], |r| r.get(0)).optional()?.flatten(),
        files: count("intel_files")?,
        symbols: count("intel_symbols")?,
        imports: count("intel_imports")?,
    })
}

#[derive(Serialize, Debug)]
pub struct SymbolHit {
    project_id: String,
    project_name: String,
    path: String,
    name: String,
    kind: String,
    line: u32,
    signature: String,
}

/// Name search ranked exact match first, then prefix, then substring; shorter names first.
pub fn symbols_query(conn: &Connection, project_id: Option<&str>, query: &str, kind: Option<&str>, limit: u32) -> Result<Vec<SymbolHit>> {
    let q = query.trim();
    let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let mut stmt = conn.prepare(
        "SELECT s.project_id, p.name, s.path, s.name, s.kind, s.line, s.signature
         FROM intel_symbols s JOIN projects p ON p.id = s.project_id
         WHERE (?1 IS NULL OR s.project_id = ?1) AND s.name LIKE ?2 ESCAPE '\\' AND (?3 IS NULL OR s.kind = ?3)
         ORDER BY CASE WHEN lower(s.name) = lower(?4) THEN 0 WHEN s.name LIKE ?5 ESCAPE '\\' THEN 1 ELSE 2 END,
                  length(s.name), s.name, s.path, s.line
         LIMIT ?6",
    )?;
    let rows = stmt.query_map(params![project_id, format!("%{escaped}%"), kind, q, format!("{escaped}%"), limit.clamp(1, 500)], |r| {
        Ok(SymbolHit {
            project_id: r.get(0)?,
            project_name: r.get(1)?,
            path: r.get(2)?,
            name: r.get(3)?,
            kind: r.get(4)?,
            line: r.get(5)?,
            signature: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn indexed_paths(conn: &Connection, project_id: &str) -> Result<Vec<(String, u64)>> {
    let mut stmt = conn.prepare("SELECT path, lines FROM intel_files WHERE project_id = ?1 ORDER BY path")?;
    let rows = stmt.query_map([project_id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn import_rows(conn: &Connection, project_id: &str) -> Result<Vec<graph::ImportRow>> {
    let mut stmt = conn.prepare("SELECT from_path, target, external FROM intel_imports WHERE project_id = ?1")?;
    let rows =
        stmt.query_map([project_id], |r| Ok(graph::ImportRow { from: r.get(0)?, target: r.get(1)?, external: r.get::<_, i64>(2)? != 0 }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn graph_for(conn: &Connection, project_id: &str, level: &str, depth: u32, externals: bool) -> Result<graph::Graph> {
    let files = indexed_paths(conn, project_id)?;
    let imports = import_rows(conn, project_id)?;
    Ok(graph::build(&files, &imports, level == "file", depth.clamp(1, 4) as usize, externals, MAX_GRAPH_NODES))
}

#[derive(Serialize, Debug)]
pub struct Reference {
    path: String,
    line: u32,
    text: String,
    is_definition: bool,
}

/// Whole-word textual references across indexed files, definitions first.
pub fn references_in(root: &Path, paths: &[String], definitions: &HashSet<(String, u32)>, name: &str) -> Result<Vec<Reference>> {
    let re = Regex::new(&format!(r"\b{}\b", regex::escape(name))).map_err(|e| AppError::Msg(e.to_string()))?;
    let mut out = vec![];
    'files: for path in paths {
        let Some(text) = read_text(&root.join(path)) else { continue };
        for (i, line) in text.lines().enumerate() {
            if line.len() < 2000 && re.is_match(line) {
                let n = i as u32 + 1;
                out.push(Reference {
                    path: path.clone(),
                    line: n,
                    text: line.trim().chars().take(240).collect(),
                    is_definition: definitions.contains(&(path.clone(), n)),
                });
                if out.len() >= MAX_REFERENCES {
                    break 'files;
                }
            }
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.is_definition));
    Ok(out)
}

#[derive(Serialize, Debug)]
pub struct ImportInfo {
    target: String,
    external: bool,
    line: u32,
}

#[derive(Serialize, Debug)]
pub struct FileContext {
    symbols: Vec<SymbolHit>,
    imports: Vec<ImportInfo>,
    imported_by: Vec<String>,
}

pub fn context_for(conn: &Connection, project_id: &str, path: &str) -> Result<FileContext> {
    let mut s = conn.prepare(
        "SELECT s.project_id, p.name, s.path, s.name, s.kind, s.line, s.signature FROM intel_symbols s
         JOIN projects p ON p.id = s.project_id WHERE s.project_id = ?1 AND s.path = ?2 ORDER BY s.line",
    )?;
    let symbols = s
        .query_map(params![project_id, path], |r| {
            Ok(SymbolHit {
                project_id: r.get(0)?,
                project_name: r.get(1)?,
                path: r.get(2)?,
                name: r.get(3)?,
                kind: r.get(4)?,
                line: r.get(5)?,
                signature: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut i = conn
        .prepare("SELECT target, external, line FROM intel_imports WHERE project_id = ?1 AND from_path = ?2 ORDER BY external, target")?;
    let imports = i
        .query_map(params![project_id, path], |r| {
            Ok(ImportInfo { target: r.get(0)?, external: r.get::<_, i64>(1)? != 0, line: r.get(2)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut b = conn.prepare(
        "SELECT DISTINCT from_path FROM intel_imports WHERE project_id = ?1 AND target = ?2 AND external = 0 ORDER BY from_path",
    )?;
    let imported_by = b.query_map(params![project_id, path], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(FileContext { symbols, imports, imported_by })
}

/// Load indexed files with their text and symbols for retrieval.
fn load_for_retrieval(root: &Path, paths: Vec<String>, symbols: Vec<(String, String, u32)>) -> Vec<context::IndexedFile> {
    let mut by_path: std::collections::HashMap<String, Vec<(String, u32)>> = Default::default();
    for (path, name, line) in symbols {
        by_path.entry(path).or_default().push((name, line));
    }
    paths
        .into_iter()
        .filter_map(|path| {
            let text = read_text(&root.join(&path))?;
            Some(context::IndexedFile { symbols: by_path.remove(&path).unwrap_or_default(), path, text })
        })
        .collect()
}

/// Indexed paths, and every symbol as (path, name, line).
type RetrievalInputs = (Vec<String>, Vec<(String, String, u32)>);

fn retrieval_inputs(conn: &Connection, project_id: &str) -> Result<RetrievalInputs> {
    let paths = indexed_paths(conn, project_id)?.into_iter().map(|(p, _)| p).collect();
    let mut stmt = conn.prepare("SELECT path, name, line FROM intel_symbols WHERE project_id = ?1")?;
    let symbols = stmt.query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok((paths, symbols))
}

const ENTRY_NAMES: &[&str] = &[
    "main.rs",
    "lib.rs",
    "main.py",
    "__main__.py",
    "manage.py",
    "app.py",
    "main.go",
    "index.ts",
    "index.tsx",
    "main.ts",
    "main.tsx",
    "index.js",
    "main.js",
    "server.ts",
    "server.js",
];

pub fn architecture_markdown(conn: &Connection, project_id: &str) -> Result<String> {
    let (name, analysis): (String, Option<String>) =
        conn.query_row("SELECT name, analysis FROM projects WHERE id = ?1", [project_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let analysis: serde_json::Value = analysis.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
    let mut modules = graph_for(conn, project_id, "module", 1, false)?;
    if modules.nodes.len() <= 2 || modules.edges.is_empty() {
        // Top-level folders with no dependencies between them say nothing; look one level deeper.
        modules = graph_for(conn, project_id, "module", 2, false)?;
    }
    let pairs = |sql: &str| -> Result<Vec<(String, u32)>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let mut l =
        conn.prepare("SELECT language, COUNT(*), SUM(lines) FROM intel_files WHERE project_id = ?1 GROUP BY language ORDER BY 3 DESC")?;
    let languages = l
        .query_map([project_id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?
        .collect::<rusqlite::Result<_>>()?;
    let entry_points = indexed_paths(conn, project_id)?
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.matches('/').count() <= 3 && ENTRY_NAMES.contains(&p.rsplit('/').next().unwrap_or("")))
        .take(12)
        .collect();
    let today = db::now();
    Ok(docs::render(&docs::DocInput {
        name: &name,
        summary: analysis.get("summary").and_then(|s| s.as_str()).unwrap_or(""),
        frameworks: analysis
            .get("frameworks")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        languages,
        modules: &modules,
        hotspots: pairs(
            "SELECT target, COUNT(DISTINCT from_path) FROM intel_imports WHERE project_id = ?1 AND external = 0
             GROUP BY target ORDER BY 2 DESC, 1 LIMIT 10",
        )?,
        entry_points,
        externals: pairs(
            "SELECT target, COUNT(DISTINCT from_path) FROM intel_imports WHERE project_id = ?1 AND external = 1
             GROUP BY target ORDER BY 2 DESC, 1 LIMIT 25",
        )?,
        symbol_count: status(conn, project_id)?.symbols as u64,
        generated_at: &today[..10],
    }))
}

/// Counts and dates from Git for the report. No names or e-mail addresses leave this function.
fn git_facts(root: &Path) -> Option<report::GitFacts> {
    let g = |args: &[&str]| crate::git::git(root, args).ok().map(|s| s.trim().to_string());
    if g(&["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return None;
    }
    let mut facts = report::GitFacts {
        branch: g(&["symbolic-ref", "--quiet", "--short", "HEAD"]).filter(|b| !b.is_empty()),
        uncommitted: g(&["status", "--porcelain"]).map(|s| s.lines().count() as u64).unwrap_or(0),
        ..Default::default()
    };
    if let Some(n) = g(&["rev-list", "--count", "HEAD"]).and_then(|n| n.parse().ok()) {
        facts.commits = n;
        facts.contributors = g(&["log", "--format=%ae"]).map(|s| s.lines().collect::<HashSet<_>>().len() as u64).unwrap_or(0);
        facts.last_commit = g(&["log", "-1", "--format=%as"]);
        facts.first_commit = g(&["log", "--reverse", "--format=%as"]).and_then(|s| s.lines().next().map(String::from));
    }
    Some(facts)
}

/// The shareable codebase report for a project, as one HTML document. See [`report`].
pub fn report_html(conn: &Connection, project_id: &str, root: &Path) -> Result<String> {
    let (name, analysis): (String, Option<String>) =
        conn.query_row("SELECT name, analysis FROM projects WHERE id = ?1", [project_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let analysis: serde_json::Value = analysis.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
    let st = status(conn, project_id)?;
    if st.files == 0 {
        return Err("Build the code index first (Intelligence → Rebuild index); the report is made from it.".into());
    }
    // The folder depth that shows the most dependencies between modules while staying readable:
    // a project whose code sits in one deep folder (src-tauri/src/…) needs more levels.
    let mut modules = graph_for(conn, project_id, "module", 1, false)?;
    for depth in 2..=3 {
        let deeper = graph_for(conn, project_id, "module", depth, false)?;
        let readable = deeper.nodes.iter().filter(|n| n.kind == "module").count() <= 36;
        if readable && deeper.edges.len() > modules.edges.len() {
            modules = deeper;
        }
    }
    let pairs = |sql: &str| -> Result<Vec<(String, u32)>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let mut l =
        conn.prepare("SELECT language, COUNT(*), SUM(lines) FROM intel_files WHERE project_id = ?1 GROUP BY language ORDER BY 3 DESC")?;
    let languages: Vec<(String, u64, u64)> = l
        .query_map([project_id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?
        .collect::<rusqlite::Result<_>>()?;
    let lines = languages.iter().map(|l| l.2).sum();
    let entry_points = indexed_paths(conn, project_id)?
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.matches('/').count() <= 3 && ENTRY_NAMES.contains(&p.rsplit('/').next().unwrap_or("")))
        .take(12)
        .collect();
    let strs = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let manifests = analysis
        .get("dependencies")
        .and_then(|d| d.as_array())
        .map(|groups| {
            groups
                .iter()
                .map(|g| {
                    let deps = g
                        .get("deps")
                        .and_then(|d| d.as_array())
                        .map(|d| d.iter().map(|x| (strs(x, "name"), strs(x, "version"))).collect())
                        .unwrap_or_default();
                    (strs(g, "manifest"), strs(g, "ecosystem"), deps)
                })
                .collect()
        })
        .unwrap_or_default();
    let findings = analysis
        .get("issues")
        .and_then(|d| d.as_array())
        .map(|issues| {
            issues
                .iter()
                .map(|i| {
                    let file = i.get("file").and_then(|f| f.as_str()).map(String::from);
                    let line = i.get("line").and_then(|f| f.as_u64()).map(|n| n as u32);
                    (strs(i, "severity"), strs(i, "kind"), strs(i, "message"), file, line)
                })
                .collect()
        })
        .unwrap_or_default();
    let now = db::now();
    Ok(report::render(&report::ReportInput {
        name: &name,
        generated_at: &now[..10],
        version: env!("CARGO_PKG_VERSION"),
        summary: analysis.get("summary").and_then(|s| s.as_str()).unwrap_or(""),
        frameworks: analysis
            .get("frameworks")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        languages,
        files: st.files as u64,
        lines,
        symbols: st.symbols as u64,
        modules: &modules,
        hotspots: pairs(
            "SELECT target, COUNT(DISTINCT from_path) FROM intel_imports WHERE project_id = ?1 AND external = 0
             GROUP BY target ORDER BY 2 DESC, 1 LIMIT 10",
        )?,
        entry_points,
        externals: pairs(
            "SELECT target, COUNT(DISTINCT from_path) FROM intel_imports WHERE project_id = ?1 AND external = 1
             GROUP BY target ORDER BY 2 DESC, 1 LIMIT 25",
        )?,
        manifests,
        findings,
        git: git_facts(root),
    }))
}

// ---------------------------------------------------------------- commands

/// Save the codebase report where the user picks in a native dialog, and optionally open it in
/// the default browser. The HTML is made and written here; the webview never handles it and
/// never names the path.
#[tauri::command]
pub async fn save_project_report<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    open: bool,
) -> Result<Option<String>> {
    let root = project_root(&state, &project_id)?;
    let (html, name) = {
        let conn = state.db.conn();
        let name: String = conn.query_row("SELECT name FROM projects WHERE id = ?1", [&project_id], |r| r.get(0))?;
        (report_html(&conn, &project_id, &root)?, name)
    };
    let suggested: String = format!("{name}-report.html").chars().filter(|c| !matches!(c, '/' | '\\' | ':')).take(120).collect();
    let dialog = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog.dialog().file().set_file_name(suggested).add_filter("HTML report", &["html"]).blocking_save_file()
    })
    .await
    .map_err(|e| AppError::Msg(format!("Could not open the save dialog: {e}")))?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| AppError::Msg(format!("Unsupported save location: {e}")))?;
    std::fs::write(&path, html)?;
    activity::log(&state.db.conn(), "index", "Saved a codebase report", Some(&project_id));
    if open {
        use tauri_plugin_opener::OpenerExt;
        let _ = app.opener().open_path(path.to_string_lossy(), None::<&str>);
    }
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[derive(Serialize, Clone)]
struct Progress<'a> {
    project_id: &'a str,
    done: usize,
    total: usize,
}

#[tauri::command]
pub async fn rebuild_index<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, project_id: String) -> Result<IndexStatus> {
    let root = project_root(&state, &project_id)?;
    let (emitter, pid) = (app.clone(), project_id.clone());
    let data = tauri::async_runtime::spawn_blocking(move || {
        build_index(&root, |done, total| {
            let _ = emitter.emit(EVENT_PROGRESS, Progress { project_id: &pid, done, total });
        })
    })
    .await
    .map_err(|e| AppError::Msg(format!("Indexing failed: {e}")))?;
    let mut conn = state.db.conn();
    store_index(&mut conn, &project_id, &data)?;
    activity::log(
        &conn,
        "index",
        &format!("Indexed {} files, {} symbols, {} imports", data.files.len(), data.symbols.len(), data.imports.len()),
        Some(&project_id),
    );
    status(&conn, &project_id)
}

#[tauri::command]
pub fn index_status(state: State<'_, AppState>, project_id: String) -> Result<IndexStatus> {
    status(&state.db.conn(), &project_id)
}

#[tauri::command]
pub fn search_symbols(
    state: State<'_, AppState>,
    project_id: Option<String>,
    query: String,
    kind: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<SymbolHit>> {
    symbols_query(&state.db.conn(), project_id.as_deref(), &query, kind.as_deref().filter(|k| !k.is_empty()), limit.unwrap_or(100))
}

#[tauri::command]
pub async fn find_references(state: State<'_, AppState>, project_id: String, name: String) -> Result<Vec<Reference>> {
    if name.trim().is_empty() {
        return Ok(vec![]);
    }
    let root = project_root(&state, &project_id)?;
    let (paths, definitions) = {
        let conn = state.db.conn();
        let paths: Vec<String> = indexed_paths(&conn, &project_id)?.into_iter().map(|(p, _)| p).collect();
        let mut stmt = conn.prepare("SELECT path, line FROM intel_symbols WHERE project_id = ?1 AND name = ?2")?;
        let defs: HashSet<(String, u32)> =
            stmt.query_map(params![project_id, name], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        (paths, defs)
    };
    tauri::async_runtime::spawn_blocking(move || references_in(&root, &paths, &definitions, &name))
        .await
        .map_err(|e| AppError::Msg(format!("Reference search failed: {e}")))?
}

#[tauri::command]
pub fn dependency_graph(
    state: State<'_, AppState>,
    project_id: String,
    level: String,
    depth: u32,
    externals: bool,
) -> Result<graph::Graph> {
    graph_for(&state.db.conn(), &project_id, &level, depth, externals)
}

#[tauri::command]
pub fn file_context(state: State<'_, AppState>, project_id: String, path: String) -> Result<FileContext> {
    context_for(&state.db.conn(), &project_id, &path)
}

/// Build the index for a project if it has never been indexed.
pub(crate) async fn ensure_index(state: &AppState, project_id: &str) -> Result<()> {
    if status(&state.db.conn(), project_id)?.indexed_at.is_some() {
        return Ok(());
    }
    let root = project_root(state, project_id)?;
    let data = tauri::async_runtime::spawn_blocking(move || build_index(&root, |_, _| {}))
        .await
        .map_err(|e| AppError::Msg(format!("Indexing failed: {e}")))?;
    store_index(&mut state.db.conn(), project_id, &data)
}

pub(crate) async fn retrieve_sources(
    state: &AppState,
    project_id: &str,
    question: String,
    focus: Option<String>,
) -> Result<Vec<context::Source>> {
    let root = project_root(state, project_id)?;
    let (paths, symbols) = retrieval_inputs(&state.db.conn(), project_id)?;
    if paths.is_empty() {
        return Err("This project has not been indexed yet. Build the index first.".into());
    }
    // Files directly connected to the focus file are always worth showing.
    let mut related: Vec<(String, &'static str)> = vec![];
    if let Some(path) = &focus {
        let ctx = context_for(&state.db.conn(), project_id, path)?;
        related.extend(ctx.imports.into_iter().filter(|i| !i.external).map(|i| (i.target, "imported by the file you asked about")));
        related.extend(ctx.imported_by.into_iter().map(|p| (p, "uses the file you asked about")));
    }
    tauri::async_runtime::spawn_blocking(move || {
        context::retrieve(&question, &load_for_retrieval(&root, paths, symbols), focus.as_deref(), &related)
    })
    .await
    .map_err(|e| AppError::Msg(format!("Retrieval failed: {e}")))
}

/// Relevant source snippets for a question. Works with no AI provider.
#[tauri::command]
pub async fn find_context(
    state: State<'_, AppState>,
    project_id: String,
    question: String,
    focus_path: Option<String>,
) -> Result<Vec<context::Source>> {
    retrieve_sources(&state, &project_id, question, focus_path).await
}

#[derive(Serialize)]
pub struct Answer {
    answer: String,
    sources: Vec<context::Source>,
    model: String,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    duration_ms: u64,
}

/// Answer a question with an AI provider, grounded in numbered snippets the answer must cite.
#[tauri::command]
pub async fn ask_codebase(
    state: State<'_, AppState>,
    project_id: String,
    question: String,
    provider_id: String,
    model: Option<String>,
    focus_path: Option<String>,
    allow_remote: Option<bool>,
) -> Result<Answer> {
    // Source excerpts leave this machine only when the user allowed it for this question.
    ai::provider_for_project_data(&state, &provider_id, allow_remote.unwrap_or(false))?;
    let sources = retrieve_sources(&state, &project_id, question.clone(), focus_path).await?;
    if sources.is_empty() {
        return Err("No source files matched this question, so there is nothing to ground an answer in. Try naming a file, function or concept from the code.".into());
    }
    // The answer cites the real excerpts; the model gets them with credentials masked.
    let masked: Vec<context::Source> =
        sources.iter().map(|s| context::Source { text: crate::projects::mask_secrets(&s.text), ..s.clone() }).collect();
    let (system, user) = context::build_prompt(&question, &masked);
    let reply =
        ai::complete(&state, &provider_id, model, Some(system), vec![ai::ChatMessage { role: "user".into(), content: user }]).await?;
    activity::log(&state.db.conn(), "ai", "Answered a codebase question with cited sources", Some(&project_id));
    Ok(Answer {
        answer: reply.content,
        sources,
        model: reply.model,
        input_tokens: reply.input_tokens,
        output_tokens: reply.output_tokens,
        duration_ms: reply.duration_ms,
    })
}

/// Architecture document. Deterministic; an AI narrative is added only when a provider is given.
#[tauri::command]
pub async fn architecture_doc(
    state: State<'_, AppState>,
    project_id: String,
    provider_id: Option<String>,
    model: Option<String>,
    allow_remote: Option<bool>,
) -> Result<String> {
    let doc = architecture_markdown(&state.db.conn(), &project_id)?;
    let Some(provider_id) = provider_id else { return Ok(doc) };
    ai::provider_for_project_data(&state, &provider_id, allow_remote.unwrap_or(false))?;
    let system = "You are a software architect. Using only the facts in the document provided, write a short narrative \
                  overview (two or three paragraphs) of how this system is organised and what a new contributor should \
                  read first. Do not invent components that are not listed."
        .to_string();
    let reply =
        ai::complete(&state, &provider_id, model, Some(system), vec![ai::ChatMessage { role: "user".into(), content: doc.clone() }])
            .await?;
    let split = doc.find("## Overview").unwrap_or(0);
    Ok(format!("{}## Narrative (AI-generated by {})\n\n{}\n\n{}", &doc[..split], reply.model, reply.content.trim(), &doc[split..]))
}

/// Save text to a location the user picks in a native dialog. The dialog is opened here, in the
/// core, so the webview can never choose which path gets written.
#[tauri::command]
pub async fn export_text<R: Runtime>(app: AppHandle<R>, suggested_name: String, content: String) -> Result<Option<String>> {
    let name: String = suggested_name.chars().filter(|c| !matches!(c, '/' | '\\' | ':')).take(120).collect();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_file_name(name).add_filter("Markdown", &["md"]).blocking_save_file()
    })
    .await
    .map_err(|e| AppError::Msg(format!("Could not open the save dialog: {e}")))?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| AppError::Msg(format!("Unsupported save location: {e}")))?;
    std::fs::write(&path, content)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::tests::temp_project;

    /// Writes a real report for a folder, through the same analysis, index and renderer the app
    /// uses. For producing the sample in docs/assets/demo:
    /// `NEXUS_REPORT_FOR=/path NEXUS_REPORT_OUT=out.html cargo test --lib write_report_for_a_folder -- --ignored`
    #[test]
    #[ignore]
    fn write_report_for_a_folder() {
        let root = std::path::PathBuf::from(std::env::var("NEXUS_REPORT_FOR").expect("NEXUS_REPORT_FOR"));
        let out = std::env::var("NEXUS_REPORT_OUT").expect("NEXUS_REPORT_OUT");
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let analysis = crate::projects::analyze(&root);
        let data = build_index(&root, |_, _| {});
        let db = db::Db::in_memory().unwrap();
        let mut conn = db.conn();
        conn.execute(
            "INSERT INTO projects (id, name, path, added_at, analysis) VALUES ('p', ?1, ?2, '', ?3)",
            params![name, root.to_string_lossy(), serde_json::to_string(&analysis).unwrap()],
        )
        .unwrap();
        store_index(&mut conn, "p", &data).unwrap();
        std::fs::write(&out, report_html(&conn, "p", &root).unwrap()).unwrap();
    }

    /// The shareable report: built from the index and analysis, never contains source text,
    /// escapes everything it prints, and cannot load or run anything.
    #[test]
    fn the_report_explains_the_project_without_its_code() {
        let (db, dir, _) = indexed();
        let analysis = serde_json::json!({
            "summary": "A demo project.",
            "frameworks": ["React"],
            "dependencies": [{"manifest": "package.json", "ecosystem": "npm", "deps": [{"name": "react", "version": "^19.0.0", "dev": false}]}],
            "issues": [
                {"severity": "info", "kind": "marker", "message": format!("{}: return secret_sauce(42)", "TO\u{44}O"), "file": "src/main.ts", "line": 3},
                {"severity": "high", "kind": "secret", "message": "Possible hard-coded credential", "file": "src/lib/format.ts", "line": 1}
            ]
        });
        db.conn()
            .execute(
                "UPDATE projects SET name = ?1, analysis = ?2 WHERE id = 'p'",
                params!["demo <script>alert(1)</script>", analysis.to_string()],
            )
            .unwrap();
        let html = report_html(&db.conn(), "p", &dir).unwrap();
        // What it shows.
        assert!(html.contains("<svg class=\"graph\"") && html.contains("src/lib"), "module graph");
        assert!(html.contains("react") && html.contains("^19.0.0"), "declared dependencies");
        assert!(html.contains("Possible hard-coded credential") && html.contains("src/lib/format.ts:1"));
        assert!(html.contains("Code marker (to-do, fix-me or hack note)") && html.contains("src/main.ts:3"));
        // What it never shows: source text, including what a finding quoted.
        for code in ["secret_sauce", "return greet", "return fmt(name)", "export function", "GREETING = "] {
            assert!(!html.contains(code), "source text leaked: {code}");
        }
        // Inert: escaped names, no scripts, nothing loaded from anywhere.
        assert!(!html.contains("<script>") && html.contains("demo &lt;script&gt;"));
        assert!(html.contains("default-src 'none'"));
        assert!(!html.contains("http://") || !html.contains("src=\"http"), "no external resources");
        // Without an index there is nothing to report.
        db.conn().execute("DELETE FROM intel_files", []).unwrap();
        assert!(report_html(&db.conn(), "p", &dir).unwrap_err().to_string().contains("Build the code index first"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A small mixed project indexed into an in-memory database.
    fn indexed() -> (db::Db, std::path::PathBuf, IndexData) {
        let dir = temp_project(&[
            ("src/main.ts", "import { greet } from './lib/greeter';\nimport React from 'react';\nexport function main() { return greet('x'); }\n"),
            ("src/lib/greeter.ts", "import { fmt } from './format';\nexport function greet(name: string) { return fmt(name); }\nexport const GREETING = 'hi';\n"),
            ("src/lib/format.ts", "export function fmt(s: string) { return s; }\n"),
            ("core/src/lib.rs", "mod engine;\npub fn start() { engine::run(); }\n"),
            ("core/src/engine.rs", "use crate::start;\npub fn run() {}\npub struct Engine;\n"),
            ("node_modules/dep/index.js", "export function hidden() {}\n"),
            ("README.md", "greet is documented here\n"),
        ]);
        let data = build_index(&dir, |_, _| {});
        let db = db::Db::in_memory().unwrap();
        {
            let mut conn = db.conn();
            conn.execute(
                "INSERT INTO projects (id, name, path, added_at, analysis) VALUES ('p', 'demo', ?1, '', ?2)",
                params![dir.to_string_lossy(), r#"{"summary":"A demo project.","frameworks":["React"]}"#],
            )
            .unwrap();
            store_index(&mut conn, "p", &data).unwrap();
        }
        (db, dir, data)
    }

    #[test]
    fn indexes_files_symbols_and_resolved_imports() {
        let (db, dir, data) = indexed();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(data.files.len(), 5, "ignored folders and non-source files are skipped");
        let imports: Vec<(&str, &str, bool)> = data.imports.iter().map(|(f, t, e, _)| (f.as_str(), t.as_str(), *e)).collect();
        assert!(imports.contains(&("src/main.ts", "src/lib/greeter.ts", false)));
        assert!(imports.contains(&("src/main.ts", "react", true)));
        assert!(imports.contains(&("core/src/lib.rs", "core/src/engine.rs", false)));
        assert!(imports.contains(&("core/src/engine.rs", "core/src/lib.rs", false)));
        let s = status(&db.conn(), "p").unwrap();
        assert!(s.indexed_at.is_some());
        assert_eq!((s.files, s.imports), (5, data.imports.len() as i64));

        // Re-indexing replaces rather than duplicates.
        store_index(&mut db.conn(), "p", &data).unwrap();
        assert_eq!(status(&db.conn(), "p").unwrap().symbols, s.symbols);
    }

    #[test]
    fn ranks_symbol_search_and_filters_by_kind() {
        let (db, dir, _) = indexed();
        std::fs::remove_dir_all(&dir).unwrap();
        let conn = db.conn();
        let names = |q: &str, kind: Option<&str>| -> Vec<String> {
            symbols_query(&conn, Some("p"), q, kind, 50).unwrap().into_iter().map(|h| h.name).collect()
        };
        assert_eq!(names("greet", None), ["greet", "GREETING"], "exact match ranks before prefix match");
        assert_eq!(names("GREET", Some("constant")), ["GREETING"]);
        assert_eq!(names("ngin", None), ["Engine"]);
        assert!(names("hidden", None).is_empty());
        assert!(names("%", None).is_empty(), "wildcards are escaped");
        assert!(symbols_query(&conn, Some("other"), "greet", None, 50).unwrap().is_empty());
    }

    #[test]
    fn finds_references_with_definitions_first() {
        let (db, dir, _) = indexed();
        let (paths, defs) = {
            let conn = db.conn();
            let paths: Vec<String> = indexed_paths(&conn, "p").unwrap().into_iter().map(|(p, _)| p).collect();
            let mut stmt = conn.prepare("SELECT path, line FROM intel_symbols WHERE project_id = 'p' AND name = 'greet'").unwrap();
            let defs: HashSet<(String, u32)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
            (paths, defs)
        };
        let refs = references_in(&dir, &paths, &defs, "greet").unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(refs.len(), 3, "{refs:?}");
        assert!(refs[0].is_definition && refs[0].path == "src/lib/greeter.ts" && refs[0].line == 2);
        assert!(refs.iter().all(|r| !r.text.contains("GREETING")), "whole-word match only");
    }

    #[test]
    fn builds_graph_context_and_architecture_doc_from_the_store() {
        let (db, dir, _) = indexed();
        std::fs::remove_dir_all(&dir).unwrap();
        let conn = db.conn();
        let g = graph_for(&conn, "p", "module", 1, true).unwrap();
        assert!(g.edges.iter().any(|e| e.from == "src" && e.to == "ext:react"));
        let files = graph_for(&conn, "p", "file", 1, false).unwrap();
        assert_eq!(files.cycles, [vec!["core/src/engine.rs".to_string(), "core/src/lib.rs".into()]]);

        let ctx = context_for(&conn, "p", "src/lib/greeter.ts").unwrap();
        assert_eq!(ctx.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["greet", "GREETING"]);
        assert_eq!(ctx.imports.iter().map(|i| i.target.as_str()).collect::<Vec<_>>(), ["src/lib/format.ts"]);
        assert_eq!(ctx.imported_by, ["src/main.ts"]);

        let doc = architecture_markdown(&conn, "p").unwrap();
        for expected in [
            "# demo — Architecture",
            "A demo project.",
            "**Stack:** React",
            "| TypeScript | 3 |",
            "`core/src/lib.rs`",
            "| `react` | 1 files |",
        ] {
            assert!(doc.contains(expected), "missing {expected:?} in:\n{doc}");
        }
    }

    #[test]
    fn retrieves_sources_for_a_question_from_disk() {
        let (db, dir, _) = indexed();
        let (paths, symbols) = retrieval_inputs(&db.conn(), "p").unwrap();
        let sources = context::retrieve("how does greet format a name?", &load_for_retrieval(&dir, paths, symbols), None, &[]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(sources[0].path, "src/lib/greeter.ts");
        assert!(sources.iter().any(|s| s.path == "src/lib/format.ts"));
        assert!(sources.iter().all(|s| !s.path.contains("node_modules")));
    }
}
