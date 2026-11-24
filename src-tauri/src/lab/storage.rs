//! The folder NEXUS downloads model files into.
//!
//! Safety rules: file names are plain names (never paths), files are only created by the
//! download manager, and the only files NEXUS will delete are ones it downloaded itself and
//! finished. Other files found in the folder are listed but never touched.

use super::{load_settings, save_settings};
use crate::error::{AppError, Result};
use crate::{activity, AppState};
use rusqlite::params;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;

pub const PART_SUFFIX: &str = ".part";

#[derive(Serialize, Debug)]
pub struct StoredFile {
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    /// Downloaded by NEXUS and complete, so NEXUS may delete it on request.
    pub managed: bool,
    /// An unfinished download.
    pub partial: bool,
}

#[derive(Serialize, Debug)]
pub struct StorageInfo {
    pub dir: String,
    pub is_default: bool,
    pub exists: bool,
    /// Free and total space of the disk holding the folder; None where the platform does not report it.
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    /// Bytes used by model files (complete and partial) in the folder.
    pub used_bytes: u64,
    pub files: Vec<StoredFile>,
}

pub fn default_dir(state: &AppState) -> PathBuf {
    state.data_dir.join("models")
}

/// The storage folder in use. The default folder is created on demand; a chosen folder is not.
pub fn dir(state: &AppState) -> Result<PathBuf> {
    match load_settings(state)?.storage_dir {
        Some(custom) => Ok(PathBuf::from(custom)),
        None => {
            let d = default_dir(state);
            fs::create_dir_all(&d)?;
            Ok(d)
        }
    }
}

/// Free and total bytes of the filesystem holding `path` (or its nearest existing parent).
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    crate::platform::disk::space(path)
}

/// Check a file name for the storage folder: a plain name ending in `.gguf`, with no path parts
/// and nothing that could be mistaken for one.
pub fn safe_name(name: &str) -> Result<String> {
    let name = name.trim();
    let ok_chars = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if name.is_empty() || name.len() > 200 || !ok_chars || name.starts_with('.') || name.contains("..") {
        return Err("Model file names may only contain letters, digits, dot, dash, underscore and plus".into());
    }
    // Names Windows reserves for devices cannot be files there, whatever the extension.
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        return Err("That file name is reserved by Windows; choose another".into());
    }
    if !name.to_ascii_lowercase().ends_with(".gguf") {
        return Err("Only .gguf model files can be stored".into());
    }
    Ok(name.to_string())
}

/// Turn the last path segment of a URL or repository path into a storable file name.
pub fn name_from_source(source: &str) -> Result<String> {
    let last = source.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
    let cleaned: String =
        last.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+') { c } else { '_' }).collect();
    safe_name(cleaned.trim_start_matches('.'))
}

fn modified(meta: &fs::Metadata) -> Option<String> {
    let time: chrono::DateTime<chrono::Utc> = meta.modified().ok()?.into();
    Some(time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Whether NEXUS downloaded this file to completion.
pub fn is_managed(state: &AppState, path: &Path) -> bool {
    state
        .db
        .conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM lab_downloads WHERE kind = 'file' AND status = 'completed' AND dest = ?1)",
            [path.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap_or(false)
}

pub fn info(state: &AppState) -> Result<StorageInfo> {
    let settings = load_settings(state)?;
    let dir = dir(state)?;
    let mut files = vec![];
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            // Symlinks are not followed: only real files in this folder count.
            let Ok(meta) = entry.metadata() else { continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            let partial = lower.ends_with(&format!(".gguf{PART_SUFFIX}"));
            if !(lower.ends_with(".gguf") || partial) {
                continue;
            }
            files.push(StoredFile {
                managed: !partial && is_managed(state, &entry.path()),
                name,
                size_bytes: meta.len(),
                modified_at: modified(&meta),
                partial,
            });
        }
    }
    files.sort_by_key(|a| a.name.to_lowercase());
    let space = disk_space(&dir);
    Ok(StorageInfo {
        dir: dir.to_string_lossy().into_owned(),
        is_default: settings.storage_dir.is_none(),
        exists: dir.is_dir(),
        free_bytes: space.map(|s| s.0),
        total_bytes: space.map(|s| s.1),
        used_bytes: files.iter().map(|f| f.size_bytes).sum(),
        files,
    })
}

/// Use `folder` for downloads from now on. Files already downloaded stay where they are.
pub fn set_dir(state: &AppState, folder: Option<&Path>) -> Result<()> {
    if state.lab.downloads.count() > 0 {
        return Err("Wait for running downloads to finish (or cancel them) before changing the folder.".into());
    }
    let mut settings = load_settings(state)?;
    settings.storage_dir = match folder {
        None => None,
        Some(path) => {
            if !path.is_absolute() || !path.is_dir() {
                return Err("Choose an existing folder".into());
            }
            let canonical = crate::platform::paths::canonical(path)?;
            // Refuse to write a probe file; just make sure it can be listed.
            fs::read_dir(&canonical).map_err(|e| AppError::Msg(format!("NEXUS cannot read that folder: {e}")))?;
            Some(canonical.to_string_lossy().into_owned())
        }
    };
    save_settings(state, &settings)?;
    activity::log(&state.db.conn(), "models", "Changed the model storage folder", None);
    Ok(())
}

/// Delete a model file NEXUS downloaded. Refuses anything else.
pub fn delete_file(state: &AppState, name: &str) -> Result<()> {
    let name = safe_name(name)?;
    let dir = dir(state)?;
    let path = dir.join(&name);
    let meta = fs::symlink_metadata(&path).map_err(|_| AppError::Msg("That file is no longer in the model folder".into()))?;
    if !meta.file_type().is_file() {
        return Err("Only regular model files can be deleted".into());
    }
    if !is_managed(state, &path) {
        return Err("NEXUS only deletes model files it downloaded itself. Remove this one with your file manager.".into());
    }
    fs::remove_file(&path)?;
    let conn = state.db.conn();
    conn.execute("DELETE FROM lab_downloads WHERE kind = 'file' AND dest = ?1", params![path.to_string_lossy()])?;
    conn.execute("DELETE FROM lab_models WHERE key = ?1", [super::registry::file_key(&name)])?;
    activity::log(&conn, "models", &format!("Deleted the model file {name}"), None);
    Ok(())
}

#[tauri::command]
pub fn lab_storage_info(state: State<'_, AppState>) -> Result<StorageInfo> {
    info(&state)
}

/// Pick the storage folder in a native dialog. The dialog is opened here, in the core, so the
/// webview can never point downloads at a path of its own choosing.
#[tauri::command]
pub async fn lab_storage_choose<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Option<StorageInfo>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title("Choose a folder for model files").blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Msg(format!("Could not open the folder dialog: {e}")))?;
    let Some(folder) = picked else { return Ok(None) };
    let path = folder.into_path().map_err(|e| AppError::Msg(format!("Unsupported folder: {e}")))?;
    set_dir(&state, Some(&path))?;
    Ok(Some(info(&state)?))
}

#[tauri::command]
pub fn lab_storage_reset(state: State<'_, AppState>) -> Result<StorageInfo> {
    set_dir(&state, None)?;
    info(&state)
}

#[tauri::command]
pub fn lab_storage_delete(state: State<'_, AppState>, name: String) -> Result<StorageInfo> {
    delete_file(&state, &name)?;
    info(&state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn completed(state: &AppState, path: &Path) {
        state.db.conn().execute(
            "INSERT INTO lab_downloads (id, kind, source, name, dest, status, created_at, updated_at) VALUES (?1, 'file', 'https://example.invalid/x.gguf', ?2, ?3, 'completed', '', '')",
            params![db::new_id(), path.file_name().unwrap().to_string_lossy(), path.to_string_lossy()],
        ).unwrap();
    }

    #[test]
    fn file_names_cannot_escape_the_folder() {
        for good in ["model.gguf", "Qwen2.5-7B+Q4_K_M.GGUF", "a_b-c.1.gguf"] {
            assert_eq!(safe_name(good).unwrap(), good);
        }
        for bad in [
            "",
            "../x.gguf",
            "a/b.gguf",
            "a\\b.gguf",
            ".hidden.gguf",
            "x..gguf",
            "model.bin",
            "model.gguf.part",
            "nul\0.gguf",
            "sp ace.gguf",
            "~x.gguf",
        ] {
            assert!(safe_name(bad).is_err(), "{bad:?} must be refused");
        }
        assert_eq!(
            name_from_source("https://host/org/repo/resolve/main/sub/Tiny%20Model.Q4.gguf?download=true").unwrap(),
            "Tiny_20Model.Q4.gguf"
        );
        assert_eq!(name_from_source("org/repo/stories260K.gguf").unwrap(), "stories260K.gguf");
        assert!(name_from_source("https://host/").is_err());
        assert!(name_from_source("https://host/readme.md").is_err());
    }

    #[test]
    fn lists_model_files_and_reports_real_disk_space() {
        let state = AppState::for_tests();
        let dir = dir(&state).unwrap();
        assert!(dir.ends_with("models") && dir.is_dir(), "default folder is created");
        fs::write(dir.join("mine.gguf"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("half.gguf.part"), vec![0u8; 300]).unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        fs::write(dir.join("downloaded.gguf"), vec![0u8; 50]).unwrap();
        completed(&state, &dir.join("downloaded.gguf"));
        let info = info(&state).unwrap();
        assert!(info.is_default && info.exists);
        let names: Vec<(&str, bool, bool)> = info.files.iter().map(|f| (f.name.as_str(), f.managed, f.partial)).collect();
        assert_eq!(names, [("downloaded.gguf", true, false), ("half.gguf.part", false, true), ("mine.gguf", false, false)]);
        assert_eq!(info.used_bytes, 1350);
        #[cfg(unix)]
        {
            let (free, total) = (info.free_bytes.unwrap(), info.total_bytes.unwrap());
            assert!(total > 0 && free <= total, "free {free} total {total}");
        }
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[test]
    fn only_files_nexus_downloaded_can_be_deleted() {
        let state = AppState::for_tests();
        let dir = dir(&state).unwrap();
        fs::write(dir.join("mine.gguf"), "user's own file").unwrap();
        fs::write(dir.join("downloaded.gguf"), "from nexus").unwrap();
        completed(&state, &dir.join("downloaded.gguf"));

        let err = delete_file(&state, "mine.gguf").unwrap_err().to_string();
        assert!(err.contains("only deletes model files it downloaded"), "{err}");
        assert!(dir.join("mine.gguf").exists());
        assert!(delete_file(&state, "../mine.gguf").is_err());
        assert!(delete_file(&state, "missing.gguf").is_err());

        // A completed download whose file was swapped for a symlink is not followed.
        #[cfg(unix)]
        {
            let outside = state.data_dir.join("outside.txt");
            fs::write(&outside, "keep me").unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("link.gguf")).unwrap();
            completed(&state, &dir.join("link.gguf"));
            assert!(delete_file(&state, "link.gguf").is_err());
            assert_eq!(fs::read_to_string(&outside).unwrap(), "keep me");
        }

        delete_file(&state, "downloaded.gguf").unwrap();
        assert!(!dir.join("downloaded.gguf").exists());
        let left: i64 =
            state.db.conn().query_row("SELECT COUNT(*) FROM lab_downloads WHERE name = 'downloaded.gguf'", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[test]
    fn storage_folder_can_be_changed_to_an_existing_folder_only() {
        let state = AppState::for_tests();
        let custom = state.data_dir.join("elsewhere");
        assert!(set_dir(&state, Some(&custom)).is_err(), "must exist");
        assert!(set_dir(&state, Some(Path::new("relative/path"))).is_err());
        fs::create_dir_all(&custom).unwrap();
        fs::write(custom.join("x.gguf"), "1234").unwrap();
        set_dir(&state, Some(&custom)).unwrap();
        let i = info(&state).unwrap();
        assert!(!i.is_default);
        assert_eq!(PathBuf::from(&i.dir), crate::platform::paths::canonical(&custom).unwrap());
        assert_eq!(i.files.len(), 1);
        // Changing folders is refused while a download is running.
        let _rx = state.lab.downloads.register("d").unwrap();
        assert!(set_dir(&state, None).is_err());
        state.lab.downloads.finish("d");
        set_dir(&state, None).unwrap();
        assert!(info(&state).unwrap().is_default);
        fs::remove_dir_all(&state.data_dir).unwrap();
    }
}
