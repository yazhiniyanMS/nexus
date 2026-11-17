//! Finding programs: Git, model runtimes and anything else NEXUS starts by name.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// File extensions that make a file runnable by name on Windows.
const WINDOWS_EXTENSIONS: [&str; 4] = ["exe", "cmd", "bat", "com"];

pub fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// File names `name` may have on this system (`git` is `git.exe` on Windows).
fn candidates(name: &str) -> Vec<String> {
    if cfg!(windows) && Path::new(name).extension().is_none() {
        WINDOWS_EXTENSIONS.iter().map(|ext| format!("{name}.{ext}")).collect()
    } else {
        vec![name.to_string()]
    }
}

/// First executable called `name` in `dirs`, then among `extra` (full paths).
pub fn find_in(name: &str, dirs: &[PathBuf], extra: &[PathBuf]) -> Option<PathBuf> {
    let names = candidates(name);
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).chain(extra.iter().cloned()).find(|p| is_executable(p))
}

/// Folders of the command search path.
pub fn search_dirs() -> Vec<PathBuf> {
    std::env::split_paths(super::env::search_path()).collect()
}

/// A program on the user's search path.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn find(name: &str) -> Option<PathBuf> {
    find_in(name, &search_dirs(), &[])
}

/// Where Git for Windows installs when it was not added to PATH.
fn git_fallbacks() -> Vec<PathBuf> {
    let mut extra = vec![];
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(base) = std::env::var_os(var) {
                extra.push(PathBuf::from(base).join("Git").join("cmd").join("git.exe"));
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            extra.push(PathBuf::from(local).join("Programs").join("Git").join("cmd").join("git.exe"));
        }
    }
    extra
}

/// The Git executable, looked up once.
pub fn git() -> Option<PathBuf> {
    static GIT: OnceLock<Option<PathBuf>> = OnceLock::new();
    GIT.get_or_init(|| find_in("git", &search_dirs(), &git_fallbacks())).clone()
}

/// Why Git could not be found, in words that say what to do on this system.
pub fn git_missing_message() -> &'static str {
    if cfg!(windows) {
        "Git was not found. Install Git for Windows (https://git-scm.com/download/win) and restart NEXUS."
    } else if cfg!(target_os = "macos") {
        "Git was not found. Install the Xcode Command Line Tools (xcode-select --install) or Git from Homebrew, then restart NEXUS."
    } else {
        "Git was not found. Install it with your package manager (for example: sudo apt install git) and restart NEXUS."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_executables_by_name_in_order_and_ignores_plain_files() {
        let dir = std::env::temp_dir().join(format!("nexus-programs-{}", crate::db::new_id()));
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let file = if cfg!(windows) { "tool.exe" } else { "tool" };
        std::fs::write(a.join(file), "").unwrap();
        std::fs::write(b.join(file), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The copy in `a` is not executable, so the one in `b` wins.
            std::fs::set_permissions(b.join(file), std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(find_in("tool", &[a.clone(), b.clone()], &[]), Some(b.join(file)));
        }
        #[cfg(windows)]
        assert_eq!(find_in("tool", &[a.clone(), b.clone()], &[]), Some(a.join(file)));
        assert_eq!(find_in("missing", &[a.clone(), b.clone()], &[]), None);
        assert_eq!(find_in("missing", &[], &[b.join(file)]), Some(b.join(file)), "explicit fallbacks are used last");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn windows_names_get_runnable_extensions() {
        if cfg!(windows) {
            assert_eq!(candidates("git"), ["git.exe", "git.cmd", "git.bat", "git.com"]);
            assert_eq!(candidates("git.exe"), ["git.exe"]);
        } else {
            assert_eq!(candidates("git"), ["git"]);
        }
    }
}
