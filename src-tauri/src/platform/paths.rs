//! Path normalisation. Windows `canonicalize` returns verbatim paths (`\\?\C:\…`) that Git and
//! people cannot use, and that never compare equal to the plain form; every canonical path in
//! NEXUS goes through [`canonical`] so one spelling is used everywhere.

use std::path::{Path, PathBuf};

/// Remove the Windows verbatim prefix where the plain form means the same thing.
/// `\\?\C:\a` becomes `C:\a`, `\\?\UNC\host\share` becomes `\\host\share`; anything else is unchanged.
pub fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        let mut chars = rest.chars();
        let drive = matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_ascii_alphabetic());
        if drive {
            return rest.to_string();
        }
    }
    path.to_string()
}

/// `canonicalize` with a spelling that is the same on every system: symlinks resolved, absolute,
/// and no verbatim prefix on Windows.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let full = path.canonicalize()?;
    Ok(if cfg!(windows) { PathBuf::from(strip_verbatim(&full.to_string_lossy())) } else { full })
}

/// A path as text for people: no verbatim prefix.
pub fn display(path: &Path) -> String {
    strip_verbatim(&path.to_string_lossy())
}

/// The user's home folder, if the system says where it is.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_removed_only_where_safe() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\dev\repo"), r"C:\Users\dev\repo");
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\repo"), r"\\server\share\repo");
        assert_eq!(strip_verbatim(r"\\?\Volume{1234}\x"), r"\\?\Volume{1234}\x", "no plain equivalent");
        assert_eq!(strip_verbatim(r"C:\already\plain"), r"C:\already\plain");
        assert_eq!(strip_verbatim("/home/dev/repo"), "/home/dev/repo");
    }

    #[test]
    fn canonical_paths_exist_and_are_absolute() {
        let dir = std::env::temp_dir();
        let c = canonical(&dir).unwrap();
        assert!(c.is_absolute() && c.is_dir());
        assert!(!c.to_string_lossy().starts_with(r"\\?\"));
        assert!(canonical(&dir.join("nexus-does-not-exist-7f3a")).is_err());
    }
}
