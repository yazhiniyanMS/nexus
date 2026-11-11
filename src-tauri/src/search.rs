//! Cross-project text search.

use crate::error::{AppError, Result};
use crate::projects::{all_projects, read_text, rel, walker};
use crate::AppState;
use regex::RegexBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

const MAX_MATCHES: usize = 500;

#[derive(Serialize)]
pub struct Match {
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
    truncated: bool,
}

fn search_project(re: &regex::Regex, id: &str, name: &str, root: &Path, out: &mut SearchResult) {
    for entry in walker(root).flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let Some(text) = read_text(entry.path()) else { continue };
        out.files_scanned += 1;
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                if out.matches.len() >= MAX_MATCHES {
                    out.truncated = true;
                    return;
                }
                out.matches.push(Match {
                    project_id: id.into(),
                    project_name: name.into(),
                    path: rel(root, entry.path()),
                    line: i as u32 + 1,
                    text: line.trim().chars().take(300).collect(),
                });
            }
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
}
