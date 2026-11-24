//! Cross-project text search.

use crate::error::{AppError, Result};
use crate::projects::{all_projects, rel, walker};
use crate::AppState;
use regex::RegexBuilder;
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tauri::State;

const MAX_MATCHES: usize = 500;

#[derive(Serialize)]
pub struct Match {
    /// "file" when the path itself matched, "content" for a matching line.
    kind: &'static str,
    project_id: String,
    project_name: String,
    path: String,
    line: u32,
    text: String,
}

#[derive(Serialize, Default)]
pub struct SearchResult {
    matches: Vec<Match>,
    files_scanned: u64,
    /// Files too large to scan.
    files_skipped: u64,
    truncated: bool,
}

/// Files larger than this are skipped (and counted) rather than scanned.
const MAX_SEARCH_BYTES: u64 = 64 * 1024 * 1024;

/// Scan one file line by line without loading it whole. Returns false once the match limit is hit.
fn search_file(re: &regex::Regex, file: &Path, mut on_match: impl FnMut(u32, &str) -> bool) -> std::io::Result<bool> {
    let mut reader = BufReader::new(File::open(file)?);
    if reader.fill_buf()?.iter().take(8000).any(|b| *b == 0) {
        return Ok(true); // binary
    }
    let mut buf = Vec::new();
    let mut line_no = 0u32;
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(true);
        }
        line_no += 1;
        // Extremely long lines are minified or generated data; matching them is slow and useless.
        if buf.len() > 20_000 {
            continue;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\n', '\r']);
        if re.is_match(line) && !on_match(line_no, line) {
            return Ok(false);
        }
    }
}

pub(crate) fn search_project(re: &regex::Regex, id: &str, name: &str, root: &Path, out: &mut SearchResult) {
    for entry in walker(root).flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        out.files_scanned += 1;
        let path = rel(root, entry.path());
        let push = |out: &mut SearchResult, kind: &'static str, line: u32, text: &str| {
            if out.matches.len() >= MAX_MATCHES {
                out.truncated = true;
                return false;
            }
            out.matches.push(Match {
                kind,
                project_id: id.into(),
                project_name: name.into(),
                path: path.clone(),
                line,
                text: text.trim().chars().take(300).collect(),
            });
            true
        };
        if re.is_match(&path) && !push(out, "file", 0, "") {
            return;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SEARCH_BYTES {
            out.files_skipped += 1;
            continue;
        }
        let complete = search_file(re, entry.path(), |line, text| push(out, "content", line, text));
        if matches!(complete, Ok(false)) {
            return;
        }
    }
}

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    regex: bool,
    case_sensitive: bool,
    project_ids: Option<Vec<String>>,
) -> Result<SearchResult> {
    if query.trim().is_empty() {
        return Ok(SearchResult::default());
    }
    let pattern = if regex { query.clone() } else { regex::escape(&query) };
    let re = RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(2_000_000)
        .build()
        .map_err(|e| AppError::Msg(format!("Invalid pattern: {e}")))?;
    let targets: Vec<(String, String, PathBuf)> = all_projects(&state.db.conn())?
        .into_iter()
        .filter(|p| p.exists && project_ids.as_ref().map(|ids| ids.contains(&p.id)).unwrap_or(true))
        .map(|p| (p.id, p.name, PathBuf::from(p.path)))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = SearchResult::default();
        for (id, name, root) in &targets {
            search_project(&re, id, name, root, &mut out);
            if out.truncated {
                break;
            }
        }
        out
    })
    .await
    .map_err(|e| AppError::Msg(format!("Search failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::tests::temp_project;

    #[test]
    fn finds_matches_and_skips_ignored_dirs() {
        let dir = temp_project(&[("a.rs", "fn needle() {}\nfn other() {}\n"), ("node_modules/b.js", "needle\n")]);
        let re = RegexBuilder::new("NEEDLE").case_insensitive(true).build().unwrap();
        let mut out = SearchResult::default();
        search_project(&re, "1", "p", &dir, &mut out);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(out.matches.len(), 1);
        assert_eq!(out.matches[0].line, 1);
        assert_eq!(out.matches[0].path, "a.rs");
    }

    #[test]
    fn matches_file_names_including_binary_files() {
        let dir = temp_project(&[("src/Widget.tsx", "export {};\n"), ("other.rs", "nothing\n")]);
        std::fs::write(dir.join("widget-logo.png"), [0u8, 1, 2]).unwrap();
        let re = RegexBuilder::new("widget").case_insensitive(true).build().unwrap();
        let mut out = SearchResult::default();
        search_project(&re, "1", "p", &dir, &mut out);
        std::fs::remove_dir_all(&dir).unwrap();
        let mut files: Vec<&str> = out.matches.iter().filter(|m| m.kind == "file").map(|m| m.path.as_str()).collect();
        files.sort();
        assert_eq!(files, ["src/Widget.tsx", "widget-logo.png"]);
        assert_eq!(out.files_scanned, 3);
    }

    #[test]
    fn searches_files_too_large_to_preview() {
        let big = "filler line\n".repeat(200_000) + "the needle is here\n"; // ~2.4 MB
        let dir = temp_project(&[("big.log", &big)]);
        let re = RegexBuilder::new("needle").build().unwrap();
        let mut out = SearchResult::default();
        search_project(&re, "1", "p", &dir, &mut out);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(out.matches.len(), 1);
        assert_eq!(out.matches[0].line, 200_001);
    }

    #[test]
    fn stops_at_the_match_limit() {
        let dir = temp_project(&[("many.txt", &"hit\n".repeat(MAX_MATCHES + 50))]);
        let re = RegexBuilder::new("hit").build().unwrap();
        let mut out = SearchResult::default();
        search_project(&re, "1", "p", &dir, &mut out);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(out.matches.len(), MAX_MATCHES);
        assert!(out.truncated);
    }
}
