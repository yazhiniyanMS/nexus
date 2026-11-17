//! Isolated workspaces. Each agent run works in its own Git worktree, checked out at the project's
//! current commit, outside the project folder. Agents never write to the project itself; the only
//! path from a workspace back to the project is `apply`, which is all-or-nothing.

use crate::error::{AppError, Result};
use crate::git::git;
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MAX_CHANGED_FILES: usize = 40;
pub const MAX_FILE_BYTES: usize = 400_000;

/// Create a detached worktree of the project's HEAD at `dest`. Returns the base commit.
pub fn create(project: &Path, dest: &Path) -> Result<String> {
    if git(project, &["rev-parse", "--is-inside-work-tree"]).map(|s| s.trim() != "true").unwrap_or(true) {
        return Err("Agents need a Git repository so they can work in an isolated copy. Run `git init` and make a first commit.".into());
    }
    let base = git(project, &["rev-parse", "--verify", "HEAD"])
        .map_err(|_| AppError::Msg("Agents need at least one commit to branch their workspace from.".into()))?
        .trim()
        .to_string();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    // Git cannot use Windows verbatim paths (\\?\C:\...), so it is given the plain spelling.
    git(project, &["worktree", "add", "--detach", &crate::platform::paths::display(dest), &base])?;
    Ok(base)
}

/// Remove a workspace. Safe to call when it no longer exists.
pub fn remove(project: &Path, dest: &Path) {
    if dest.exists() && git(project, &["worktree", "remove", "--force", &crate::platform::paths::display(dest)]).is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    let _ = git(project, &["worktree", "prune"]);
}

/// Resolve a model-supplied relative path inside `root`, refusing anything that could escape it.
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let refuse = |why: &str| AppError::Msg(format!("Refused unsafe path “{relative}”: {why}"));
    let rel = relative.trim();
    if rel.is_empty() || rel.contains('\0') {
        return Err(refuse("empty or invalid"));
    }
    let candidate = Path::new(rel);
    let mut clean = PathBuf::new();
    for part in candidate.components() {
        match part {
            Component::Normal(p) => {
                if p.to_string_lossy().eq_ignore_ascii_case(".git") {
                    return Err(refuse("Git metadata is off limits"));
                }
                clean.push(p);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(refuse("it climbs out of the workspace")),
            Component::RootDir | Component::Prefix(_) => return Err(refuse("absolute paths are not allowed")),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(refuse("empty or invalid"));
    }
    // No component on the way may be a symlink: a link could point outside the workspace.
    let root = crate::platform::paths::canonical(root)?;
    let mut walked = root.clone();
    for part in clean.components() {
        walked.push(part);
        if fs::symlink_metadata(&walked).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            return Err(refuse("it passes through a symbolic link"));
        }
    }
    Ok(root.join(clean))
}

pub fn write_file(root: &Path, relative: &str, content: &str) -> Result<()> {
    if content.len() > MAX_FILE_BYTES {
        return Err(AppError::Msg(format!("Refused to write “{relative}”: file is larger than {} KB", MAX_FILE_BYTES / 1000)));
    }
    let path = safe_join(root, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        // Belt and braces: the real parent must still be inside the workspace.
        if !crate::platform::paths::canonical(parent)?.starts_with(crate::platform::paths::canonical(&root)?) {
            return Err(AppError::Msg(format!("Refused unsafe path “{relative}”: it resolves outside the workspace")));
        }
    }
    fs::write(path, content)?;
    Ok(())
}

/// Read a workspace file for model context. Missing, binary or oversized files yield None.
pub fn read_file(root: &Path, relative: &str, max_bytes: usize) -> Option<String> {
    let path = safe_join(root, relative).ok()?;
    let bytes = fs::read(path).ok()?;
    if bytes.len() > max_bytes || bytes.iter().take(8000).any(|b| *b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ChangedFile {
    /// "added", "modified" or "deleted".
    pub status: String,
    pub path: String,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Diff {
    pub patch: String,
    pub files: Vec<ChangedFile>,
}

/// Everything the workspace changed relative to its base commit, as one patch.
pub fn diff(root: &Path) -> Result<Diff> {
    // The engine and the UI both ask for diffs; Git allows one index writer at a time.
    static INDEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    // Staging here touches only the worktree's own index, never the project's.
    git(root, &["add", "-A"])?;
    let patch = git(root, &["diff", "--cached", "--binary", "--no-color", "HEAD"])?;
    let files = git(root, &["diff", "--cached", "--name-status", "HEAD"])?
        .lines()
        .filter_map(|l| {
            let (status, path) = l.split_once('\t')?;
            let status = match status.chars().next()? {
                'A' => "added",
                'D' => "deleted",
                _ => "modified",
            };
            Some(ChangedFile { status: status.into(), path: path.rsplit('\t').next()?.to_string() })
        })
        .collect();
    Ok(Diff { patch, files })
}

/// Apply a patch to the project's working tree, or change nothing at all.
/// Fails without touching anything if it would collide with the user's own uncommitted edits.
pub fn apply(project: &Path, patch: &str, scratch: &Path) -> Result<()> {
    if patch.trim().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(scratch)?;
    let file = scratch.join(format!("apply-{}.patch", crate::db::new_id()));
    fs::write(&file, patch)?;
    let path = file.to_string_lossy().into_owned();
    let result = git(project, &["apply", "--check", "--whitespace=nowarn", &path])
        .map_err(|e| {
            AppError::Msg(format!(
                "These changes do not apply cleanly to your project, so nothing was changed. \
                 This usually means you edited the same files since the agent started. Details: {e}"
            ))
        })
        .and_then(|_| git(project, &["apply", "--whitespace=nowarn", &path]).map(|_| ()));
    let _ = fs::remove_file(&file);
    result
}

/// Test commands implied by the workspace's manifests. Deterministic; no AI involved.
pub fn discover_tests(dir: &Path) -> Vec<String> {
    let has = |f: &str| dir.join(f).exists();
    let read = |f: &str| fs::read_to_string(dir.join(f)).unwrap_or_default();
    let mut out = vec![];
    if has("package.json") {
        let pm = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "yarn"
        } else {
            "npm"
        };
        let scripts = serde_json::from_str::<serde_json::Value>(&read("package.json"))
            .ok()
            .and_then(|v| v.get("scripts").and_then(|s| s.as_object()).map(|m| m.keys().cloned().collect::<Vec<_>>()))
            .unwrap_or_default();
        if scripts.iter().any(|s| s == "test") {
            if !has("node_modules") {
                out.push(format!("{pm} install"));
            }
            out.push(format!("{pm} test"));
        }
    }
    for (manifest, flag) in [("Cargo.toml", ""), ("src-tauri/Cargo.toml", " --manifest-path src-tauri/Cargo.toml")] {
        if has(manifest) {
            out.push(format!("cargo test{flag}"));
            break;
        }
    }
    if has("go.mod") {
        out.push("go test ./...".into());
    }
    let python = if cfg!(windows) { "python" } else { "python3" };
    let has_py_tests = has("tests")
        || fs::read_dir(dir)
            .map(|d| {
                d.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.starts_with("test_") && n.ends_with(".py")
                })
            })
            .unwrap_or(false);
    if has("pyproject.toml") || has("requirements.txt") || has_py_tests {
        if read("pyproject.toml").contains("pytest") || read("requirements.txt").contains("pytest") || has("pytest.ini") {
            out.push(format!("{python} -m pytest"));
        } else if has_py_tests {
            out.push(format!("{python} -m unittest discover"));
        }
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::projects::tests::temp_project;

    /// A committed Git repository in a temp folder.
    pub fn repo(files: &[(&str, &str)]) -> PathBuf {
        let dir = crate::platform::paths::canonical(&temp_project(files)).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]).unwrap();
        git(&dir, &["add", "-A"]).unwrap();
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base"]).unwrap();
        dir
    }

    #[test]
    fn refuses_paths_that_escape_the_workspace() {
        let dir = repo(&[("src/a.txt", "one\n")]);
        let outside = dir.parent().unwrap().join(format!("outside-{}", crate::db::new_id()));
        fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();

        for bad in ["../escape.txt", "src/../../escape.txt", "/etc/passwd", ".git/config", "src/.GIT/hooks/x", "", "  "] {
            assert!(write_file(&dir, bad, "x").is_err(), "{bad:?} must be refused");
        }
        #[cfg(unix)]
        {
            assert!(write_file(&dir, "link/planted.txt", "x").unwrap_err().to_string().contains("symbolic link"));
            assert!(fs::read_dir(&outside).unwrap().next().is_none(), "nothing may be written outside");
        }
        assert!(write_file(&dir, "big.txt", &"x".repeat(MAX_FILE_BYTES + 1)).is_err());
        write_file(&dir, "src/new/deep.txt", "ok").unwrap();
        assert_eq!(fs::read_to_string(dir.join("src/new/deep.txt")).unwrap(), "ok");
        assert_eq!(read_file(&dir, "src/a.txt", 100).as_deref(), Some("one\n"));
        assert_eq!(read_file(&dir, "../x", 100), None);
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn worktree_is_isolated_and_apply_is_all_or_nothing() {
        let project = repo(&[("a.txt", "one\ntwo\nthree\n"), ("keep.txt", "mine\n")]);
        let ws = project.parent().unwrap().join(format!("ws-{}", crate::db::new_id()));
        // The user has uncommitted work in the project.
        fs::write(project.join("keep.txt"), "mine, edited but not committed\n").unwrap();

        let base = create(&project, &ws).unwrap();
        assert_eq!(base.len(), 40);
        assert_eq!(
            fs::read_to_string(ws.join("keep.txt")).unwrap(),
            "mine\n",
            "workspace starts from the commit, not from uncommitted edits"
        );

        write_file(&ws, "a.txt", "one\nTWO\nthree\n").unwrap();
        write_file(&ws, "new.txt", "hello\n").unwrap();
        let d = diff(&ws).unwrap();
        assert_eq!(
            d.files,
            [
                ChangedFile { status: "modified".into(), path: "a.txt".into() },
                ChangedFile { status: "added".into(), path: "new.txt".into() }
            ]
        );
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "project untouched before apply");
        assert!(!project.join("new.txt").exists());

        apply(&project, &d.patch, &ws.parent().unwrap().join("scratch")).unwrap();
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "one\nTWO\nthree\n");
        assert_eq!(fs::read_to_string(project.join("new.txt")).unwrap(), "hello\n");
        assert_eq!(fs::read_to_string(project.join("keep.txt")).unwrap(), "mine, edited but not committed\n", "user's edits survive");
        assert!(git(&project, &["diff", "--cached", "--name-only"]).unwrap().trim().is_empty(), "nothing is staged or committed");

        // A second patch that collides with a local edit must change nothing.
        fs::write(project.join("a.txt"), "completely different local edit\n").unwrap();
        write_file(&ws, "a.txt", "one\nTWO!\nthree\n").unwrap();
        write_file(&ws, "second.txt", "x\n").unwrap();
        let err = apply(&project, &diff(&ws).unwrap().patch, &ws.parent().unwrap().join("scratch")).unwrap_err().to_string();
        assert!(err.contains("nothing was changed"), "{err}");
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "completely different local edit\n");
        assert!(!project.join("second.txt").exists(), "a failed apply must not partially apply");

        remove(&project, &ws);
        assert!(!ws.exists());
        fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn concurrent_diffs_do_not_collide_on_the_git_index() {
        let project = repo(&[("a.txt", "one\n")]);
        let ws = project.parent().unwrap().join(format!("ws-{}", crate::db::new_id()));
        create(&project, &ws).unwrap();
        write_file(&ws, "a.txt", "two\n").unwrap();
        let handles: Vec<_> = (0..12)
            .map(|_| {
                let ws = ws.clone();
                std::thread::spawn(move || (0..5).map(|_| diff(&ws).map(|d| d.files.len())).collect::<Vec<_>>())
            })
            .collect();
        for h in handles {
            for result in h.join().unwrap() {
                assert_eq!(result.unwrap(), 1);
            }
        }
        remove(&project, &ws);
        fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn requires_a_repository_with_a_commit_and_discovers_tests() {
        let plain = temp_project(&[("a.txt", "x")]);
        assert!(create(&plain, &plain.join("ws")).unwrap_err().to_string().contains("Git repository"));
        git(&plain, &["init", "-q"]).unwrap();
        assert!(create(&plain, &plain.join("ws")).unwrap_err().to_string().contains("at least one commit"));

        let dir = temp_project(&[("package.json", r#"{"scripts":{"test":"vitest"}}"#), ("src-tauri/Cargo.toml", ""), ("test_x.py", "")]);
        let python = if cfg!(windows) { "python" } else { "python3" };
        assert_eq!(
            discover_tests(&dir),
            ["npm install", "npm test", "cargo test --manifest-path src-tauri/Cargo.toml", &format!("{python} -m unittest discover")]
        );
        assert!(discover_tests(&plain).is_empty());
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&plain).unwrap();
    }
}
