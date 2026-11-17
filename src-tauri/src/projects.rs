//! Project intelligence: import local repositories and analyse them without any AI.

use crate::error::{AppError, Result};
use crate::{activity, db, platform, AppState};
use ignore::WalkBuilder;
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::State;

/// Directories that are never worth analysing or searching.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
    ".gradle",
    ".turbo",
    ".cache",
    "coverage",
    ".svelte-kit",
    "Pods",
];
const MAX_FILES: usize = 60_000;
const MAX_TEXT_BYTES: u64 = 1_500_000;
const MAX_ISSUES_PER_KIND: usize = 40;

/// Walk a project honouring .gitignore and skipping well-known generated directories.
pub fn walker(root: &Path) -> ignore::Walk {
    WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            !(is_dir && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .build()
}

/// Read a file as text, returning None for binary, oversized or unreadable files.
pub fn read_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_TEXT_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Display a path relative to the project root using forward slashes on every OS.
pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn language_for(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy();
    match name.as_ref() {
        "Dockerfile" => return Some("Dockerfile"),
        "Makefile" | "makefile" => return Some("Makefile"),
        "CMakeLists.txt" => return Some("CMake"),
        _ => {}
    }
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "rs" => "Rust",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TypeScript (TSX)",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JavaScript (JSX)",
        "py" | "pyi" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "lua" | "luau" => "Lua",
        "dart" => "Dart",
        "scala" => "Scala",
        "ex" | "exs" => "Elixir",
        "hs" => "Haskell",
        "zig" => "Zig",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" | "less" => "Sass/Less",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "md" | "mdx" => "Markdown",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "proto" => "Protocol Buffers",
        "graphql" | "gql" => "GraphQL",
        "tf" => "Terraform",
        _ => return None,
    })
}

/// Languages that count as data/markup and should not be picked as the "primary" language.
fn is_markup(lang: &str) -> bool {
    matches!(lang, "Markdown" | "JSON" | "YAML" | "TOML" | "XML" | "HTML" | "CSS" | "Sass/Less")
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LanguageStat {
    pub name: String,
    pub files: u64,
    pub lines: u64,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub dev: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DependencyGroup {
    pub manifest: String,
    pub ecosystem: String,
    pub deps: Vec<Dependency>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Issue {
    pub severity: String,
    pub kind: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DirStat {
    pub name: String,
    pub files: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Analysis {
    pub file_count: u64,
    pub total_lines: u64,
    pub total_bytes: u64,
    pub languages: Vec<LanguageStat>,
    pub frameworks: Vec<String>,
    pub dependencies: Vec<DependencyGroup>,
    pub issues: Vec<Issue>,
    pub top_dirs: Vec<DirStat>,
    pub summary: String,
    pub truncated: bool,
}

fn secret_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            (Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(), "Possible AWS access key"),
            (Regex::new(r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----").unwrap(), "Private key material"),
            (
                Regex::new(r#"(?i)(api[_-]?key|secret|passwd|password|auth[_-]?token)["']?\s*[:=]\s*["'][A-Za-z0-9_\-/+]{20,}["']"#)
                    .unwrap(),
                "Possible hard-coded credential",
            ),
        ]
    })
}

fn marker_pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| Regex::new(r"\b(TODO|FIXME|HACK|XXX)\b").unwrap())
}

struct IssueSink {
    issues: Vec<Issue>,
    counts: HashMap<&'static str, usize>,
}

impl IssueSink {
    fn push(&mut self, severity: &str, kind: &'static str, message: String, file: Option<String>, line: Option<u32>) {
        let n = self.counts.entry(kind).or_insert(0);
        *n += 1;
        if *n <= MAX_ISSUES_PER_KIND {
            self.issues.push(Issue { severity: severity.into(), kind: kind.into(), message, file, line });
        }
    }
}

fn parse_package_json(text: &str) -> Vec<Dependency> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return vec![] };
    let mut out = vec![];
    for (key, dev) in [("dependencies", false), ("devDependencies", true), ("peerDependencies", false)] {
        if let Some(map) = v.get(key).and_then(|d| d.as_object()) {
            for (name, ver) in map {
                out.push(Dependency { name: name.clone(), version: ver.as_str().unwrap_or("").into(), dev });
            }
        }
    }
    out
}

fn parse_cargo_toml(text: &str) -> Vec<Dependency> {
    let Ok(v) = text.parse::<toml::Table>() else { return vec![] };
    let mut out = vec![];
    for (key, dev) in [("dependencies", false), ("dev-dependencies", true), ("build-dependencies", true)] {
        if let Some(map) = v.get(key).and_then(|d| d.as_table()) {
            for (name, spec) in map {
                let version = match spec {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Table(t) => t
                        .get("version")
                        .and_then(|x| x.as_str())
                        .map(String::from)
                        .or_else(|| t.get("path").map(|_| "path".to_string()))
                        .or_else(|| t.get("git").map(|_| "git".to_string()))
                        .or_else(|| t.get("workspace").map(|_| "workspace".to_string()))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                out.push(Dependency { name: name.clone(), version, dev });
            }
        }
    }
    out
}

fn split_requirement(line: &str) -> Option<Dependency> {
    let line = line.split('#').next()?.trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }
    let idx = line.find(|c: char| "=<>!~;[ @".contains(c)).unwrap_or(line.len());
    let name = line[..idx].trim();
    if name.is_empty() {
        return None;
    }
    let version = line[idx..].split(';').next().unwrap_or("").trim().to_string();
    Some(Dependency { name: name.to_string(), version, dev: false })
}

fn parse_requirements(text: &str) -> Vec<Dependency> {
    text.lines().filter_map(split_requirement).collect()
}

fn parse_pyproject(text: &str) -> Vec<Dependency> {
    let Ok(v) = text.parse::<toml::Table>() else { return vec![] };
    let mut out = vec![];
    if let Some(list) = v.get("project").and_then(|p| p.get("dependencies")).and_then(|d| d.as_array()) {
        out.extend(list.iter().filter_map(|x| x.as_str()).filter_map(split_requirement));
    }
    if let Some(map) = v.get("tool").and_then(|t| t.get("poetry")).and_then(|p| p.get("dependencies")).and_then(|d| d.as_table()) {
        for (name, spec) in map {
            if name != "python" {
                out.push(Dependency { name: name.clone(), version: spec.as_str().unwrap_or("").to_string(), dev: false });
            }
        }
    }
    out
}

fn parse_go_mod(text: &str) -> Vec<Dependency> {
    let mut out = vec![];
    let mut in_block = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && line == ")" {
            in_block = false;
            continue;
        }
        let body = if in_block {
            line
        } else if let Some(r) = line.strip_prefix("require ") {
            r
        } else {
            continue;
        };
        let mut parts = body.split_whitespace();
        if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
            out.push(Dependency { name: name.into(), version: ver.into(), dev: body.contains("// indirect") });
        }
    }
    out
}

/// Map well-known dependency names to the framework/tool they indicate.
const FRAMEWORK_HINTS: &[(&str, &str)] = &[
    ("next", "Next.js"),
    ("react", "React"),
    ("vue", "Vue"),
    ("svelte", "Svelte"),
    ("@angular/core", "Angular"),
    ("express", "Express"),
    ("fastify", "Fastify"),
    ("@nestjs/core", "NestJS"),
    ("vite", "Vite"),
    ("tailwindcss", "Tailwind CSS"),
    ("electron", "Electron"),
    ("@tauri-apps/api", "Tauri"),
    ("tauri", "Tauri"),
    ("typescript", "TypeScript"),
    ("jest", "Jest"),
    ("vitest", "Vitest"),
    ("axum", "Axum"),
    ("actix-web", "Actix Web"),
    ("tokio", "Tokio"),
    ("bevy", "Bevy"),
    ("django", "Django"),
    ("flask", "Flask"),
    ("fastapi", "FastAPI"),
    ("torch", "PyTorch"),
    ("tensorflow", "TensorFlow"),
    ("pandas", "pandas"),
    ("pytest", "pytest"),
    ("github.com/gin-gonic/gin", "Gin"),
    ("rails", "Rails"),
    ("prisma", "Prisma"),
];

pub fn analyze(root: &Path) -> Analysis {
    let mut langs: BTreeMap<&'static str, LanguageStat> = BTreeMap::new();
    let mut dirs: BTreeMap<String, u64> = BTreeMap::new();
    let mut frameworks: BTreeSet<String> = BTreeSet::new();
    let mut dependencies = vec![];
    let mut sink = IssueSink { issues: vec![], counts: HashMap::new() };
    let mut a = Analysis::default();
    let (mut has_readme, mut has_license, mut has_tests, mut has_ci) = (false, false, false, false);

    for entry in walker(root).flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if a.file_count as usize >= MAX_FILES {
            a.truncated = true;
            break;
        }
        let path = entry.path();
        let relp = rel(root, path);
        let depth = relp.matches('/').count();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let lower = name.to_lowercase();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        a.file_count += 1;
        a.total_bytes += size;

        let top = if depth == 0 { "(root)".to_string() } else { relp.split('/').next().unwrap_or("").to_string() };
        *dirs.entry(top).or_insert(0) += 1;

        if depth == 0 && lower.starts_with("readme") {
            has_readme = true;
        }
        if depth == 0 && (lower.starts_with("license") || lower.starts_with("copying")) {
            has_license = true;
        }
        if relp.starts_with(".github/workflows/") || lower == ".gitlab-ci.yml" {
            has_ci = true;
        }
        if relp.contains("test") || relp.contains("spec") {
            has_tests = true;
        }
        if lower == ".env" || (lower.starts_with(".env.") && !lower.ends_with(".example") && !lower.ends_with(".sample")) {
            sink.push("high", "env-file", "Environment file is not git-ignored; it may contain secrets".into(), Some(relp.clone()), None);
        }
        if lower == "dockerfile" || lower.starts_with("docker-compose") {
            frameworks.insert("Docker".into());
        }

        let lang = language_for(path);
        let text = if lang.is_some() || depth <= 3 { read_text(path) } else { None };

        // Manifests: dependency mapping + framework detection (shallow only, to skip vendored copies).
        if depth <= 3 {
            if let Some(text) = &text {
                let parsed = match name.as_str() {
                    "package.json" => Some(("npm", parse_package_json(text))),
                    "Cargo.toml" => Some(("cargo", parse_cargo_toml(text))),
                    "requirements.txt" => Some(("pip", parse_requirements(text))),
                    "pyproject.toml" => Some(("pip", parse_pyproject(text))),
                    "go.mod" => Some(("go", parse_go_mod(text))),
                    _ => None,
                };
                if let Some((eco, deps)) = parsed {
                    for d in &deps {
                        if let Some((_, fw)) = FRAMEWORK_HINTS.iter().find(|(k, _)| *k == d.name.to_lowercase()) {
                            frameworks.insert((*fw).to_string());
                        }
                    }
                    if !deps.is_empty() {
                        dependencies.push(DependencyGroup { manifest: relp.clone(), ecosystem: eco.into(), deps });
                    }
                }
            }
        }

        let Some(lang) = lang else { continue };
        let stat = langs.entry(lang).or_insert_with(|| LanguageStat { name: lang.into(), ..Default::default() });
        stat.files += 1;
        stat.bytes += size;
        let Some(text) = text else {
            if size > MAX_TEXT_BYTES {
                sink.push("low", "large-file", format!("Very large file ({} KB)", size / 1024), Some(relp.clone()), None);
            }
            continue;
        };
        let mut lines = 0u64;
        let scan = !is_markup(lang) && !lower.ends_with(".min.js") && !lower.contains("lock");
        for (i, line) in text.lines().enumerate() {
            lines += 1;
            if !scan || line.len() > 2000 {
                continue;
            }
            if let Some(m) = marker_pattern().find(line) {
                let snippet: String = line.trim().chars().take(140).collect();
                sink.push("info", "marker", format!("{}: {}", m.as_str(), snippet), Some(relp.clone()), Some(i as u32 + 1));
            }
            for (re, label) in secret_patterns() {
                if re.is_match(line) {
                    sink.push("high", "secret", (*label).to_string(), Some(relp.clone()), Some(i as u32 + 1));
                }
            }
        }
        stat.lines += lines;
        a.total_lines += lines;
        if scan && lines > 1000 {
            sink.push("medium", "long-file", format!("{lines} lines; consider splitting this file"), Some(relp.clone()), None);
        }
    }

    if !has_readme {
        sink.push("medium", "missing-readme", "No README found at the project root".into(), None, None);
    }
    if !has_license {
        sink.push("low", "missing-license", "No LICENSE file found at the project root".into(), None, None);
    }
    if !has_tests && a.file_count > 0 {
        sink.push("medium", "missing-tests", "No test files or directories detected".into(), None, None);
    }
    if !has_ci && a.file_count > 0 {
        sink.push("low", "missing-ci", "No CI configuration detected".into(), None, None);
    }
    if has_ci {
        frameworks.insert("CI".into());
    }
    for (kind, n) in &sink.counts {
        if *n > MAX_ISSUES_PER_KIND {
            sink.issues.push(Issue {
                severity: "info".into(),
                kind: (*kind).into(),
                message: format!("{} more '{}' findings not shown", n - MAX_ISSUES_PER_KIND, kind),
                file: None,
                line: None,
            });
        }
    }
    let rank = |s: &str| match s {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    };
    sink.issues.sort_by_key(|i| rank(&i.severity));

    let mut languages: Vec<LanguageStat> = langs.into_values().collect();
    languages.sort_by(|x, y| y.lines.cmp(&x.lines));
    let mut top_dirs: Vec<DirStat> = dirs.into_iter().map(|(name, files)| DirStat { name, files }).collect();
    top_dirs.sort_by(|x, y| y.files.cmp(&x.files));
    top_dirs.truncate(12);

    a.languages = languages;
    a.frameworks = frameworks.into_iter().collect();
    a.dependencies = dependencies;
    a.issues = sink.issues;
    a.top_dirs = top_dirs;
    a.summary = summarize(root, &a);
    a
}

fn summarize(root: &Path, a: &Analysis) -> String {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "This project".into());
    if a.file_count == 0 {
        return format!("{name} contains no analysable files.");
    }
    let primary = a.languages.iter().find(|l| !is_markup(&l.name)).or(a.languages.first());
    let mut s = match primary {
        Some(l) => format!("{name} is primarily a {} codebase", l.name),
        None => format!("{name} has no recognised source code"),
    };
    let fw: Vec<&str> = a.frameworks.iter().map(String::as_str).filter(|f| *f != "CI").collect();
    if !fw.is_empty() {
        s.push_str(&format!(" using {}", fw.join(", ")));
    }
    s.push_str(&format!(". It has {} files and about {} lines across {} languages.", a.file_count, a.total_lines, a.languages.len()));
    let dep_count: usize = a.dependencies.iter().map(|g| g.deps.len()).sum();
    if dep_count > 0 {
        s.push_str(&format!(" {} dependencies are declared in {} manifest(s).", dep_count, a.dependencies.len()));
    }
    if let Some(d) = a.top_dirs.iter().find(|d| d.name != "(root)") {
        s.push_str(&format!(" Most files live in `{}/`.", d.name));
    }
    let high = a.issues.iter().filter(|i| i.severity == "high").count();
    let medium = a.issues.iter().filter(|i| i.severity == "medium").count();
    if high + medium > 0 {
        s.push_str(&format!(" Analysis flagged {high} high and {medium} medium priority findings."));
    }
    if a.truncated {
        s.push_str(" The project is very large, so analysis stopped early.");
    }
    s
}

// ---------------------------------------------------------------- commands

#[derive(Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub added_at: String,
    pub analyzed_at: Option<String>,
    pub analysis: Option<serde_json::Value>,
    pub exists: bool,
}

fn row_to_project(r: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let path: String = r.get(2)?;
    let analysis: Option<String> = r.get(5)?;
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        exists: Path::new(&path).is_dir(),
        path,
        added_at: r.get(3)?,
        analyzed_at: r.get(4)?,
        analysis: analysis.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

const PROJECT_COLS: &str = "id, name, path, added_at, analyzed_at, analysis";

pub fn all_projects(conn: &rusqlite::Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(&format!("SELECT {PROJECT_COLS} FROM projects ORDER BY name COLLATE NOCASE"))?;
    let rows = stmt.query_map([], row_to_project)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Resolve a project's root directory from its id.
pub fn project_root(state: &AppState, id: &str) -> Result<PathBuf> {
    let path: Option<String> = state.db.conn().query_row("SELECT path FROM projects WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    let path = PathBuf::from(path.ok_or("Project not found")?);
    if !path.is_dir() {
        return Err(AppError::Msg(format!("Project folder no longer exists: {}", path.display())));
    }
    Ok(path)
}

/// Resolve a user-supplied relative path, refusing anything that escapes the project root.
pub(crate) fn resolve_inside(root: &Path, relative: &str) -> Result<PathBuf> {
    let root = platform::paths::canonical(root)?;
    let full = platform::paths::canonical(&root.join(relative))?;
    if !full.starts_with(&root) {
        return Err("Path is outside the project".into());
    }
    Ok(full)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>> {
    all_projects(&state.db.conn())
}

#[tauri::command]
pub async fn add_project(state: State<'_, AppState>, path: String) -> Result<Project> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err("That path is not a folder".into());
    }
    // One spelling on every system (no Windows verbatim prefix), so paths stay usable by Git.
    let dir = platform::paths::canonical(&dir)?;
    let path_str = dir.to_string_lossy().into_owned();
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path_str.clone());
    let id = db::new_id();
    {
        let conn = state.db.conn();
        let exists: Option<String> = conn.query_row("SELECT id FROM projects WHERE path = ?1", [&path_str], |r| r.get(0)).optional()?;
        if exists.is_some() {
            return Err("This folder is already imported".into());
        }
        conn.execute("INSERT INTO projects (id, name, path, added_at) VALUES (?1, ?2, ?3, ?4)", params![id, name, path_str, db::now()])?;
        activity::log(&conn, "project", &format!("Imported project {name}"), Some(&id));
    }
    analyze_project(state, id).await
}

#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, id: String) -> Result<()> {
    let conn = state.db.conn();
    let name: Option<String> = conn.query_row("SELECT name FROM projects WHERE id = ?1", [&id], |r| r.get(0)).optional()?;
    conn.execute("DELETE FROM projects WHERE id = ?1", [&id])?;
    if let Some(name) = name {
        activity::log(&conn, "project", &format!("Removed project {name} (files untouched)"), None);
    }
    Ok(())
}

#[tauri::command]
pub async fn analyze_project(state: State<'_, AppState>, id: String) -> Result<Project> {
    let root = project_root(&state, &id)?;
    let analysis =
        tauri::async_runtime::spawn_blocking(move || analyze(&root)).await.map_err(|e| AppError::Msg(format!("Analysis failed: {e}")))?;
    let conn = state.db.conn();
    conn.execute(
        "UPDATE projects SET analysis = ?1, analyzed_at = ?2 WHERE id = ?3",
        params![serde_json::to_string(&analysis)?, db::now(), id],
    )?;
    activity::log(&conn, "analysis", &format!("Analysed {} files, {} findings", analysis.file_count, analysis.issues.len()), Some(&id));
    Ok(conn.query_row(&format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?1"), [&id], row_to_project)?)
}

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

#[tauri::command]
pub fn list_dir(state: State<'_, AppState>, project_id: String, path: String) -> Result<Vec<DirEntry>> {
    let root = platform::paths::canonical(&project_root(&state, &project_id)?)?;
    let dir = resolve_inside(&root, &path)?;
    let mut out = vec![];
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        out.push(DirEntry { path: rel(&root, &entry.path()), name, is_dir: meta.is_dir(), size: meta.len() });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// How much of a large file is shown before the preview is cut off.
const PREVIEW_BYTES: u64 = 512 * 1024;

#[derive(Serialize, Debug)]
pub struct FilePreview {
    pub content: String,
    pub size: u64,
    /// True when only the beginning of the file is included.
    pub truncated: bool,
}

fn human_size(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} bytes"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// Read the start of a file for display. Large files are cut at a line boundary; binary files are refused.
pub fn preview_file(file: &Path) -> Result<FilePreview> {
    use std::io::Read;
    let size = fs::metadata(file)?.len();
    let mut bytes = Vec::new();
    fs::File::open(file)?.take(PREVIEW_BYTES).read_to_end(&mut bytes)?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Err(AppError::Msg(format!("This is a binary file ({}), so it cannot be shown as text.", human_size(size))));
    }
    let truncated = size > bytes.len() as u64;
    if truncated {
        // Drop the partial last line (and any split multi-byte character with it).
        if let Some(pos) = bytes.iter().rposition(|b| *b == b'\n') {
            bytes.truncate(pos + 1);
        }
    }
    Ok(FilePreview { content: String::from_utf8_lossy(&bytes).into_owned(), size, truncated })
}

#[tauri::command]
pub fn read_file(state: State<'_, AppState>, project_id: String, path: String) -> Result<FilePreview> {
    let root = project_root(&state, &project_id)?;
    let file = resolve_inside(&root, &path).map_err(|e| match e {
        AppError::Io(ref io) if io.kind() == std::io::ErrorKind::NotFound => {
            AppError::Msg(format!("{path} no longer exists. It may have been moved or deleted."))
        }
        other => other,
    })?;
    if !file.is_file() {
        return Err("This is a folder, not a file.".into());
    }
    preview_file(&file).map_err(|e| match e {
        AppError::Io(ref io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
            AppError::Msg(format!("NEXUS does not have permission to read {path}."))
        }
        other => other,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn temp_project(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nexus-test-{}", db::new_id()));
        for (name, content) in files {
            let p = dir.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        dir
    }

    #[test]
    fn analyzes_a_mixed_project() {
        let dir = temp_project(&[
            ("package.json", r#"{"dependencies":{"react":"^19"},"devDependencies":{"vite":"^6"}}"#),
            ("src/app.tsx", "export const a = 1;\n// TODO: finish\n"),
            ("src-tauri/Cargo.toml", "[dependencies]\ntauri = { version = \"2\" }\nserde = \"1\"\n"),
            ("src-tauri/src/main.rs", "fn main() {}\n"),
            ("node_modules/x/index.js", "ignored();\n"),
            ("config.py", "API_KEY = \"abcdefghijklmnopqrstuvwxyz123456\"\n"),
        ]);
        let a = analyze(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(a.file_count, 5, "node_modules must be skipped");
        assert!(a.frameworks.contains(&"React".to_string()));
        assert!(a.frameworks.contains(&"Tauri".to_string()));
        assert_eq!(a.dependencies.len(), 2);
        assert!(a.issues.iter().any(|i| i.kind == "secret" && i.file.as_deref() == Some("config.py")));
        assert!(a.issues.iter().any(|i| i.kind == "marker" && i.line == Some(2)));
        assert!(a.issues.iter().any(|i| i.kind == "missing-readme"));
        assert!(a.summary.contains("React"));
    }

    #[test]
    fn parses_manifests() {
        assert_eq!(parse_requirements("flask>=2.0\n# c\n-r other.txt\nrequests\n").len(), 2);
        let go = parse_go_mod("module x\nrequire (\n\tgithub.com/a/b v1.2.3\n)\nrequire github.com/c/d v0.1.0\n");
        assert_eq!(go.len(), 2);
        assert_eq!(go[0].version, "v1.2.3");
    }

    #[test]
    fn refuses_path_escape() {
        let dir = temp_project(&[("a.txt", "x")]);
        assert!(resolve_inside(&dir, "a.txt").is_ok());
        assert!(resolve_inside(&dir, "../").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn previews_text_truncates_large_and_refuses_binary() {
        let big = "line of text\n".repeat(60_000); // ~780 KB
        let dir = temp_project(&[("small.rs", "fn main() {}\n"), ("big.log", &big)]);
        fs::write(dir.join("image.bin"), [0x89u8, b'P', b'N', b'G', 0, 0, 1, 2]).unwrap();

        let small = preview_file(&dir.join("small.rs")).unwrap();
        assert_eq!((small.content.as_str(), small.truncated), ("fn main() {}\n", false));

        let large = preview_file(&dir.join("big.log")).unwrap();
        assert!(large.truncated);
        assert_eq!(large.size, big.len() as u64);
        assert!(large.content.len() as u64 <= PREVIEW_BYTES && large.content.ends_with("line of text\n"));

        let err = preview_file(&dir.join("image.bin")).unwrap_err().to_string();
        assert!(err.contains("binary file"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
