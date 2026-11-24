//! Git intelligence: shells out to the user's `git` so behaviour matches their CLI.
//! It reads, stages and unstages, and commits what the user staged. It never discards work,
//! rewrites history, switches branches or talks to a remote.

use crate::error::{AppError, Result};
use crate::platform;
use crate::projects::project_root;
use crate::AppState;
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use tauri::State;

const MAX_DIFF_BYTES: usize = 400_000;

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let program = platform::programs::git().ok_or(platform::programs::git_missing_message())?;
    let mut cmd = Command::new(program);
    cmd.arg("-C").arg(dir);
    // A repository's own config is project content, not something the user ran. NEXUS runs
    // Git automatically (Overview, Git screen), so settings that make Git start another program
    // while only reading are switched off: the filesystem monitor hook here, external diff tools
    // and text conversion filters in `diff_args`. Hooks still run on commit, as they do for
    // `git commit` in a shell, because the user asked for that commit.
    cmd.args(["-c", "core.fsmonitor=false"]);
    cmd.args(args).env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C");
    // Never wait for a password or passphrase prompt that nobody can see.
    cmd.env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null());
    // Hooks, credential helpers and pagers Git starts need the user's search path too.
    cmd.env("PATH", platform::env::search_path());
    platform::process::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::Msg(platform::programs::git_missing_message().into())
        } else {
            AppError::Io(e)
        }
    })?;
    if !out.status.success() {
        return Err(AppError::Msg(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_DIFF_BYTES {
        let mut cut = MAX_DIFF_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… output truncated …\n");
    }
    s
}

#[derive(Serialize, Debug, PartialEq)]
pub struct FileStatus {
    path: String,
    orig_path: Option<String>,
    /// Status letter in the index ('.' when unchanged).
    index: String,
    /// Status letter in the working tree ('.' when unchanged).
    worktree: String,
    untracked: bool,
    conflicted: bool,
}

#[derive(Serialize, Default, Debug)]
pub struct Status {
    is_repo: bool,
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    files: Vec<FileStatus>,
    /// A merge is in progress (MERGE_HEAD exists).
    merging: bool,
    /// A rebase is in progress.
    rebasing: bool,
    /// The repository has at least one commit.
    has_commits: bool,
}

/// Parse `git status --porcelain=v2 --branch` output.
fn parse_status(text: &str) -> Status {
    let mut s = Status { is_repo: true, ..Default::default() };
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("# branch.head ") {
            s.branch = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("# branch.ab ") {
            let mut p = v.split_whitespace();
            s.ahead = p.next().and_then(|x| x.trim_start_matches('+').parse().ok()).unwrap_or(0);
            s.behind = p.next().and_then(|x| x.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if let Some(path) = line.strip_prefix("? ") {
            s.files.push(FileStatus {
                path: path.into(),
                orig_path: None,
                index: ".".into(),
                worktree: "?".into(),
                untracked: true,
                conflicted: false,
            });
        } else {
            let (kind, fields) = match line.as_bytes().first() {
                Some(b'1') => ('1', 9),
                Some(b'2') => ('2', 10),
                Some(b'u') => ('u', 11),
                _ => continue,
            };
            let parts: Vec<&str> = line.splitn(fields, ' ').collect();
            if parts.len() < fields {
                continue;
            }
            let xy = parts[1];
            let (path, orig) = match parts[fields - 1].split_once('\t') {
                Some((p, o)) => (p.to_string(), Some(o.to_string())),
                None => (parts[fields - 1].to_string(), None),
            };
            s.files.push(FileStatus {
                path,
                orig_path: orig,
                index: xy.chars().next().unwrap_or('.').to_string(),
                worktree: xy.chars().nth(1).unwrap_or('.').to_string(),
                untracked: false,
                conflicted: kind == 'u',
            });
        }
    }
    s
}

fn is_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).map(|s| s.trim() == "true").unwrap_or(false)
}

#[tauri::command]
pub async fn git_status(state: State<'_, AppState>, project_id: String) -> Result<Status> {
    status_of(&project_root(&state, &project_id)?)
}

pub(crate) fn status_of(root: &Path) -> Result<Status> {
    if !is_repo(root) {
        return Ok(Status::default());
    }
    let mut s = parse_status(&git(root, &["status", "--porcelain=v2", "--branch", "--untracked-files=all"])?);
    fill_state(root, &mut s);
    Ok(s)
}

/// Merge, rebase and first-commit state, which porcelain status does not report.
fn fill_state(root: &Path, s: &mut Status) {
    s.has_commits = git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
    s.merging = git(root, &["rev-parse", "--verify", "-q", "MERGE_HEAD"]).is_ok();
    let dir_exists = |name: &str| git(root, &["rev-parse", "--git-path", name]).map(|p| root.join(p.trim()).exists()).unwrap_or(false);
    s.rebasing = dir_exists("rebase-merge") || dir_exists("rebase-apply");
}

#[derive(Serialize, Debug)]
pub struct Committed {
    hash: String,
    short: String,
    subject: String,
}

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Commit what is staged, with the user's message, identity and hooks. Refuses to commit with
/// unresolved conflicts or nothing staged, and never adds files on its own.
fn commit_staged(root: &Path, message: &str) -> Result<Committed> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message first".into());
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return Err("The commit message is too long".into());
    }
    let status = parse_status(&git(root, &["status", "--porcelain=v2", "--untracked-files=no"])?);
    if status.files.iter().any(|f| f.conflicted) {
        return Err("Resolve the merge conflicts and stage the resolved files before committing".into());
    }
    if !status.files.iter().any(|f| f.index != ".") {
        return Err("Nothing is staged. Stage the changes you want in this commit first.".into());
    }
    git(root, &["commit", "--quiet", "--cleanup=strip", "-m", message]).map_err(|e| {
        let text = e.to_string();
        if text.contains("Please tell me who you are") || text.contains("user.email") {
            AppError::Msg("Git does not know who you are. Set user.name and user.email (git config --global) and try again.".into())
        } else {
            AppError::Msg(format!("Git refused the commit: {text}"))
        }
    })?;
    let out = git(root, &["log", "-1", "--format=%H%x00%h%x00%s"])?;
    let mut parts = out.trim_end().split('\0');
    Ok(Committed {
        hash: parts.next().unwrap_or_default().to_string(),
        short: parts.next().unwrap_or_default().to_string(),
        subject: parts.next().unwrap_or_default().to_string(),
    })
}

#[tauri::command]
pub async fn git_commit(state: State<'_, AppState>, project_id: String, message: String) -> Result<Committed> {
    let root = project_root(&state, &project_id)?;
    let done = commit_staged(&root, &message)?;
    crate::activity::log(&state.db.conn(), "git", &format!("Committed {} “{}”", done.short, done.subject), Some(&project_id));
    Ok(done)
}

#[derive(Serialize)]
pub struct Branch {
    name: String,
    commit: String,
    upstream: Option<String>,
    date: String,
    current: bool,
    remote: bool,
    subject: String,
}

#[tauri::command]
pub async fn git_branches(state: State<'_, AppState>, project_id: String) -> Result<Vec<Branch>> {
    let root = project_root(&state, &project_id)?;
    let out = git(
        &root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%09%(refname:short)%09%(objectname:short)%09%(upstream:short)%09%(committerdate:iso-strict)%09%(HEAD)%09%(subject)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.splitn(7, '\t').collect();
            if p.len() < 7 || p[0].ends_with("/HEAD") {
                return None;
            }
            Some(Branch {
                remote: p[0].starts_with("refs/remotes/"),
                name: p[1].into(),
                commit: p[2].into(),
                upstream: (!p[3].is_empty()).then(|| p[3].to_string()),
                date: p[4].into(),
                current: p[5] == "*",
                subject: p[6].into(),
            })
        })
        .collect())
}

#[derive(Serialize)]
pub struct Commit {
    hash: String,
    short: String,
    author: String,
    email: String,
    date: String,
    subject: String,
    refs: String,
}

#[tauri::command]
pub async fn git_log(state: State<'_, AppState>, project_id: String, limit: Option<u32>) -> Result<Vec<Commit>> {
    let root = project_root(&state, &project_id)?;
    let n = format!("-n{}", limit.unwrap_or(100).min(1000));
    let out = match git(&root, &["log", &n, "--pretty=format:%H%x1f%h%x1f%an%x1f%ae%x1f%aI%x1f%s%x1f%D%x1e"]) {
        Ok(o) => o,
        // A repository with no commits yet is not an error worth surfacing.
        Err(AppError::Msg(m)) if m.contains("does not have any commits") => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(out
        .split('\x1e')
        .filter_map(|rec| {
            let p: Vec<&str> = rec.trim_start_matches('\n').split('\x1f').collect();
            (p.len() >= 7).then(|| Commit {
                hash: p[0].into(),
                short: p[1].into(),
                author: p[2].into(),
                email: p[3].into(),
                date: p[4].into(),
                subject: p[5].into(),
                refs: p[6].into(),
            })
        })
        .collect())
}

#[tauri::command]
pub async fn git_diff(state: State<'_, AppState>, project_id: String, path: String, staged: bool, untracked: bool) -> Result<String> {
    diff_text(&project_root(&state, &project_id)?, &path, staged, untracked)
}

fn diff_text(root: &Path, path: &str, staged: bool, untracked: bool) -> Result<String> {
    if untracked {
        // Untracked files have no diff; show them as an all-added patch. The path comes from
        // the UI, so it must resolve inside the project like any other file read.
        let file = crate::projects::resolve_inside(root, path)?;
        let text = crate::projects::read_text(&file).ok_or("File is binary or too large to preview")?;
        let body: String = text.lines().map(|l| format!("+{l}\n")).collect();
        return Ok(truncate(format!("new file: {path}\n{body}")));
    }
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    Ok(truncate(git(root, &args)?))
}

#[tauri::command]
pub async fn git_show(state: State<'_, AppState>, project_id: String, hash: String) -> Result<String> {
    if hash.len() < 4 || hash.len() > 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid commit hash".into());
    }
    let root = project_root(&state, &project_id)?;
    Ok(truncate(git(&root, &["show", "--no-color", "--no-ext-diff", "--no-textconv", "--stat", "--patch", "--format=fuller", &hash])?))
}

fn stage_paths(root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(root, &args).map(|_| ())
}

/// Remove paths from the index only. The working tree is never touched, so no edits can be lost.
fn unstage_paths(root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let has_head = git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
    let mut args = if has_head { vec!["restore", "--staged", "--"] } else { vec!["rm", "--cached", "-q", "--"] };
    args.extend(paths.iter().map(String::as_str));
    git(root, &args).map(|_| ())
}

#[tauri::command]
pub async fn git_stage(state: State<'_, AppState>, project_id: String, paths: Vec<String>) -> Result<()> {
    stage_paths(&project_root(&state, &project_id)?, &paths)
}

#[tauri::command]
pub async fn git_unstage(state: State<'_, AppState>, project_id: String, paths: Vec<String>) -> Result<()> {
    unstage_paths(&project_root(&state, &project_id)?, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let s = parse_status(
            "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n\
             1 .M N... 100644 100644 100644 aaa bbb src/a file.rs\n\
             2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\told.rs\n\
             u UU N... 100644 100644 100644 100644 a b c conflict.rs\n\
             ? notes.txt\n",
        );
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.files.len(), 4);
        assert_eq!(s.files[0].path, "src/a file.rs");
        assert_eq!(s.files[0].worktree, "M");
        assert_eq!(s.files[1].orig_path.as_deref(), Some("old.rs"));
        assert!(s.files[2].conflicted);
        assert!(s.files[3].untracked);
    }
    /// Runs the real git binary against a throwaway repository.
    #[test]
    fn reads_a_real_repository() {
        let dir = crate::projects::tests::temp_project(&[("a.txt", "one\n")]);
        let g = |args: &[&str]| git(&dir, args).unwrap();
        g(&["init", "-q", "-b", "main"]);
        g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "first"]);
        assert!(is_repo(&dir));
        let s = parse_status(&g(&["status", "--porcelain=v2", "--branch", "--untracked-files=all"]));
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.files.len(), 1);
        assert!(s.files[0].untracked);
        g(&["add", "a.txt"]);
        let s = parse_status(&g(&["status", "--porcelain=v2", "--branch"]));
        assert_eq!((s.files[0].index.as_str(), s.files[0].path.as_str()), ("A", "a.txt"));
        assert!(g(&["diff", "--cached", "--", "a.txt"]).contains("+one"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn staging_and_unstaging_never_change_the_working_tree() {
        let dir = crate::projects::tests::temp_project(&[("a.txt", "one\n"), ("gone.txt", "bye\n")]);
        let g = |args: &[&str]| git(&dir, args).unwrap();
        let status = || parse_status(&g(&["status", "--porcelain=v2", "--untracked-files=all"]));
        let entry = |s: &Status, p: &str| s.files.iter().find(|f| f.path == p).map(|f| (f.index.clone(), f.worktree.clone()));
        g(&["init", "-q", "-b", "main"]);

        // Before the first commit: stage then unstage a new file.
        stage_paths(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(entry(&status(), "a.txt"), Some(("A".into(), ".".into())));
        unstage_paths(&dir, &["a.txt".into()]).unwrap();
        assert!(status().files.iter().all(|f| f.untracked));
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");

        g(&["add", "."]);
        g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "first"]);

        // After a commit: an edit and a deletion.
        std::fs::write(dir.join("a.txt"), "one\ntwo, unsaved work\n").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();
        stage_paths(&dir, &["a.txt".into(), "gone.txt".into()]).unwrap();
        let s = status();
        assert_eq!(entry(&s, "a.txt"), Some(("M".into(), ".".into())));
        assert_eq!(entry(&s, "gone.txt"), Some(("D".into(), ".".into())));

        unstage_paths(&dir, &["a.txt".into(), "gone.txt".into()]).unwrap();
        let s = status();
        assert_eq!(entry(&s, "a.txt"), Some((".".into(), "M".into())));
        assert_eq!(entry(&s, "gone.txt"), Some((".".into(), "D".into())));
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\ntwo, unsaved work\n", "edits must survive");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A project folder can arrive with its own `.git/config` (an archive, a shared drive).
    /// Looking at it in NEXUS must not run the programs that config names.
    #[cfg(unix)]
    #[test]
    fn repository_config_cannot_make_reading_run_programs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::projects::tests::temp_project(&[("a.txt", "one\n"), (".gitattributes", "*.txt diff=conv\n")]);
        let marker = dir.join("ran.marker");
        let script = dir.join("helper.sh");
        std::fs::write(&script, format!("#!/bin/sh\necho \"$0 $@\" >> '{}'\ncat \"$1\" 2>/dev/null\n", marker.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let g = |args: &[&str]| git(&dir, args).unwrap();
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "user.email", "t@example.invalid"]);
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "first"]);
        let helper = script.to_string_lossy().to_string();
        g(&["config", "core.fsmonitor", &helper]);
        g(&["config", "diff.conv.textconv", &helper]);
        g(&["config", "diff.external", &helper]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();

        let s = status_of(&dir).unwrap();
        assert!(s.files.iter().any(|f| f.path == "a.txt"));
        assert!(diff_text(&dir, "a.txt", false, false).unwrap().contains("+two"), "a plain diff is still shown");
        let head = g(&["rev-parse", "HEAD"]);
        git(&dir, &["show", "--no-color", "--no-ext-diff", "--no-textconv", "--stat", "--patch", head.trim()]).unwrap();
        assert!(!marker.exists(), "a repository-configured program ran: {:?}", std::fs::read_to_string(&marker));
        // Control: the same repository does run the helper for a plain `git diff`, so the
        // assertion above is testing something real.
        let program = platform::programs::git().unwrap();
        Command::new(program).arg("-C").arg(&dir).args(["diff"]).output().unwrap();
        assert!(marker.exists(), "control: plain git should have run the configured helper");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn untracked_previews_stay_inside_the_project() {
        let outer = crate::projects::tests::temp_project(&[("secret.txt", "outside\n"), ("proj/a.txt", "inside\n")]);
        let dir = outer.join("proj");
        assert!(diff_text(&dir, "a.txt", false, true).unwrap().contains("+inside"));
        for bad in ["../secret.txt", outer.join("secret.txt").to_str().unwrap()] {
            let err = diff_text(&dir, bad, false, true).unwrap_err().to_string();
            assert!(err.contains("outside the project"), "{bad}: {err}");
        }
        std::fs::remove_dir_all(&outer).unwrap();
    }

    #[test]
    fn commits_only_what_is_staged_and_refuses_unsafe_commits() {
        let dir = crate::projects::tests::temp_project(&[("a.txt", "one\n"), ("b.txt", "keep me\n")]);
        let g = |args: &[&str]| git(&dir, args).unwrap();
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.name", "Tester"]);
        g(&["config", "user.email", "tester@example.invalid"]);
        let state = || {
            let mut s = parse_status(&g(&["status", "--porcelain=v2", "--branch", "--untracked-files=all"]));
            fill_state(&dir, &mut s);
            s
        };
        assert!(!state().has_commits);
        assert!(commit_staged(&dir, "first").unwrap_err().to_string().contains("Nothing is staged"));
        stage_paths(&dir, &["a.txt".into()]).unwrap();
        assert!(commit_staged(&dir, "   \n ").unwrap_err().to_string().contains("message"));

        let c = commit_staged(&dir, "  Add a\n\nWith a body.  ").unwrap();
        assert_eq!(c.subject, "Add a");
        assert!(c.short.len() >= 7);
        let s = state();
        assert!(s.has_commits && !s.merging && !s.rebasing);
        // b.txt was never staged, so it is still untracked and untouched.
        assert_eq!(s.files.iter().map(|f| (f.path.as_str(), f.untracked)).collect::<Vec<_>>(), [("b.txt", true)]);
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "keep me\n");
        assert_eq!(g(&["log", "-1", "--format=%B"]).trim(), "Add a\n\nWith a body.");

        // A merge conflict blocks committing and is reported as a merge in progress.
        g(&["checkout", "-q", "-b", "other"]);
        std::fs::write(dir.join("a.txt"), "other\n").unwrap();
        g(&["commit", "-qam", "other change"]);
        g(&["checkout", "-q", "main"]);
        std::fs::write(dir.join("a.txt"), "main\n").unwrap();
        g(&["commit", "-qam", "main change"]);
        assert!(git(&dir, &["merge", "-q", "other"]).is_err());
        let s = state();
        assert!(s.merging);
        assert!(s.files.iter().any(|f| f.path == "a.txt" && f.conflicted));
        assert!(commit_staged(&dir, "merge").unwrap_err().to_string().contains("conflicts"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
