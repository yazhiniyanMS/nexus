//! Where NEXUS keeps its own files.
//!
//! | System | Default location |
//! | --- | --- |
//! | macOS | `~/Library/Application Support/dev.nexus.workspace` |
//! | Windows | `%APPDATA%\dev.nexus.workspace` |
//! | Linux | `$XDG_DATA_HOME/dev.nexus.workspace` (usually `~/.local/share/…`) |
//!
//! Setting `NEXUS_DATA_DIR` to an absolute path uses that folder instead. That is how a portable
//! copy, a second profile or a test run keeps away from the main data.

use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

pub const OVERRIDE_VAR: &str = "NEXUS_DATA_DIR";

/// The folder named by `NEXUS_DATA_DIR`, when it is set to an absolute path.
pub fn override_dir() -> Option<PathBuf> {
    parse_override(std::env::var_os(OVERRIDE_VAR))
}

fn parse_override(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value.filter(|v| !v.is_empty())?);
    path.is_absolute().then_some(path)
}

/// The data directory, created if needed.
pub fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = match override_dir() {
        Some(dir) => dir,
        None => app.path().app_data_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_absolute_override_is_used() {
        assert_eq!(parse_override(None), None);
        assert_eq!(parse_override(Some("".into())), None);
        assert_eq!(parse_override(Some("relative/dir".into())), None);
        let abs = std::env::temp_dir().join("nexus-portable");
        assert_eq!(parse_override(Some(abs.clone().into_os_string())), Some(abs));
    }
}
