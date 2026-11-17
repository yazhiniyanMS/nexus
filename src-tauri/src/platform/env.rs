//! The command search path. A desktop app started from Finder or a Linux launcher gets a short
//! PATH without Homebrew, cargo, nvm and the like, so `npm test` would not be found even though it
//! works in the user's terminal. On Unix the user's login shell is asked once for its PATH; on
//! Windows desktop apps already inherit the user's PATH, so it is used as it is.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

static SEARCH_PATH: OnceLock<OsString> = OnceLock::new();

/// Resolve the search path in the background so the first command does not wait for it.
pub fn warm_up() {
    std::thread::spawn(|| {
        search_path();
    });
}

/// PATH for programs NEXUS starts and looks up: the login shell's PATH, then this process's own,
/// then the usual install folders.
pub fn search_path() -> &'static OsString {
    SEARCH_PATH.get_or_init(|| {
        let own: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        let merged = merge(login_shell_path().unwrap_or_default(), own, common_dirs());
        std::env::join_paths(merged).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
    })
}

/// Join path lists in priority order without duplicates or empty entries.
fn merge(first: Vec<PathBuf>, second: Vec<PathBuf>, third: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    first.into_iter().chain(second).chain(third).filter(|p| !p.as_os_str().is_empty() && seen.insert(p.clone())).collect()
}

/// Install folders checked even when no PATH mentions them.
fn common_dirs() -> Vec<PathBuf> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut dirs = vec![];
    #[cfg(unix)]
    {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            dirs.push(PathBuf::from(d));
        }
        if let Some(home) = super::paths::home() {
            for d in [".cargo/bin", ".local/bin", "bin"] {
                dirs.push(home.join(d));
            }
        }
    }
    dirs
}

const MARK_START: &str = "__NEXUS_PATH_START__";
const MARK_END: &str = "__NEXUS_PATH_END__";

/// Pull the PATH out of what the shell printed; startup files may print other text around it.
#[cfg_attr(not(any(unix, test)), allow(dead_code))]
fn parse_marked(output: &str) -> Option<Vec<PathBuf>> {
    let start = output.rfind(MARK_START)? + MARK_START.len();
    let end = output[start..].find(MARK_END)? + start;
    let value = output[start..end].trim();
    (!value.is_empty()).then(|| std::env::split_paths(value).collect())
}

#[cfg(unix)]
fn login_shell_path() -> Option<Vec<PathBuf>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let shell = super::shell::login_shell();
    let name = std::path::Path::new(&shell).file_name()?.to_string_lossy().into_owned();
    // fish keeps PATH as a list; the others as one colon-separated string.
    let script = match name.as_str() {
        "fish" => format!("printf '%s' {MARK_START}; string join : $PATH; printf '%s' {MARK_END}"),
        "zsh" | "bash" | "ksh" => format!("printf '%s%s%s' {MARK_START} \"$PATH\" {MARK_END}"),
        _ => return None,
    };
    let mut child = Command::new(&shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "dumb")
        .env_remove("NEXUS_E2E_PORT")
        .spawn()
        .ok()?;
    // A slow or broken startup file must not hang the app: give the shell a few seconds.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    parse_marked(&reader.join().ok()?)
}

#[cfg(not(unix))]
fn login_shell_path() -> Option<Vec<PathBuf>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_found_between_the_markers_whatever_else_was_printed() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let noisy = format!("Welcome!\n{MARK_START}/a/bin{sep}/b/bin{MARK_END}\nbye");
        assert_eq!(parse_marked(&noisy).unwrap(), [PathBuf::from("/a/bin"), PathBuf::from("/b/bin")]);
        assert!(parse_marked("no markers").is_none());
        assert!(parse_marked(&format!("{MARK_START}{MARK_END}")).is_none());
    }

    #[test]
    fn merging_keeps_priority_order_and_drops_duplicates() {
        let p = |s: &str| PathBuf::from(s);
        let merged = merge(vec![p("/login"), p("/shared")], vec![p("/own"), p("/shared"), p("")], vec![p("/common"), p("/login")]);
        assert_eq!(merged, [p("/login"), p("/shared"), p("/own"), p("/common")]);
    }

    #[test]
    fn the_search_path_contains_this_process_path() {
        let resolved: Vec<PathBuf> = std::env::split_paths(search_path()).collect();
        assert!(!resolved.is_empty());
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|d| !d.as_os_str().is_empty()) {
            assert!(resolved.contains(&dir), "{} was dropped", dir.display());
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_login_shell_reports_a_path_with_system_folders() {
        // Skipped quietly for shells that are not asked (plain sh on a minimal CI image).
        if let Some(path) = login_shell_path() {
            assert!(path.iter().any(|d| d == std::path::Path::new("/usr/bin") || d == std::path::Path::new("/bin")), "{path:?}");
        }
    }
}
