//! Application settings: small JSON documents stored by key in SQLite. Each feature owns its
//! key and its typed struct; this module only reads and writes them.

use crate::db;
use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// The stored value for `key`, or the type's default when nothing (or something unreadable,
/// for example from a newer version) is stored.
pub fn get<T: DeserializeOwned + Default>(conn: &Connection, key: &str) -> Result<T> {
    let raw: Option<String> = conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?;
    Ok(raw.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

pub fn set<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, serde_json::to_string(value)?, db::now()],
    )?;
    Ok(())
}

// ------------------------------------------------------------------ application preferences

const PREFS_KEY: &str = "app";

/// Screens a preference may name. Anything else is refused.
pub const VIEWS: [&str; 11] =
    ["dashboard", "projects", "intel", "agents", "terminal", "models", "search", "git", "tasks", "activity", "settings"];

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AppPrefs {
    /// "dashboard" or "last": which screen the app opens on.
    pub start_view: String,
    /// The screen open when the app was last used.
    pub last_view: Option<String>,
}

impl Default for AppPrefs {
    fn default() -> Self {
        AppPrefs { start_view: "dashboard".into(), last_view: None }
    }
}

fn load_prefs(conn: &Connection) -> Result<AppPrefs> {
    let mut p: AppPrefs = get(conn, PREFS_KEY)?;
    if !matches!(p.start_view.as_str(), "dashboard" | "last") {
        p.start_view = "dashboard".into();
    }
    p.last_view = p.last_view.filter(|v| VIEWS.contains(&v.as_str()));
    Ok(p)
}

#[tauri::command]
pub fn app_prefs_get(state: tauri::State<'_, crate::AppState>) -> Result<AppPrefs> {
    load_prefs(&state.db.conn())
}

#[tauri::command]
pub fn app_prefs_set_start(state: tauri::State<'_, crate::AppState>, start_view: String) -> Result<AppPrefs> {
    if !matches!(start_view.as_str(), "dashboard" | "last") {
        return Err("Choose Overview or the last screen you used".into());
    }
    let conn = state.db.conn();
    let mut p = load_prefs(&conn)?;
    p.start_view = start_view;
    set(&conn, PREFS_KEY, &p)?;
    Ok(p)
}

#[tauri::command]
pub fn app_prefs_set_last_view(state: tauri::State<'_, crate::AppState>, view: String) -> Result<()> {
    if !VIEWS.contains(&view.as_str()) {
        return Err("Unknown screen".into());
    }
    let conn = state.db.conn();
    let mut p = load_prefs(&conn)?;
    p.last_view = Some(view);
    set(&conn, PREFS_KEY, &p)
}

/// Show the welcome screen again on the next start.
#[tauri::command]
pub fn onboarding_reset(state: tauri::State<'_, crate::AppState>) -> Result<()> {
    state.db.conn().execute("DELETE FROM settings WHERE key = 'onboarding'", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    #[serde(default)]
    struct Sample {
        name: Option<String>,
        count: u32,
    }

    #[test]
    fn settings_round_trip_and_fall_back_to_defaults() {
        let db = db::Db::in_memory().unwrap();
        let conn = db.conn();
        assert_eq!(get::<Sample>(&conn, "sample").unwrap(), Sample::default());
        set(&conn, "sample", &Sample { name: Some("a".into()), count: 3 }).unwrap();
        set(&conn, "sample", &Sample { name: Some("b".into()), count: 4 }).unwrap();
        assert_eq!(get::<Sample>(&conn, "sample").unwrap(), Sample { name: Some("b".into()), count: 4 });
        // A value written by a different version must not break reading.
        conn.execute("UPDATE settings SET value = 'not json' WHERE key = 'sample'", []).unwrap();
        assert_eq!(get::<Sample>(&conn, "sample").unwrap(), Sample::default());
        // Unknown fields are ignored and missing ones take their defaults.
        conn.execute("UPDATE settings SET value = '{\"count\": 9, \"future\": true}' WHERE key = 'sample'", []).unwrap();
        assert_eq!(get::<Sample>(&conn, "sample").unwrap(), Sample { name: None, count: 9 });
    }

    #[test]
    fn app_preferences_are_validated_and_persist() {
        let db = db::Db::in_memory().unwrap();
        let conn = db.conn();
        assert_eq!(load_prefs(&conn).unwrap(), AppPrefs::default());
        set(&conn, PREFS_KEY, &AppPrefs { start_view: "last".into(), last_view: Some("git".into()) }).unwrap();
        assert_eq!(load_prefs(&conn).unwrap(), AppPrefs { start_view: "last".into(), last_view: Some("git".into()) });
        // Values written by another version, or tampered with, fall back to safe defaults.
        set(&conn, PREFS_KEY, &AppPrefs { start_view: "nowhere".into(), last_view: Some("../../etc".into()) }).unwrap();
        assert_eq!(load_prefs(&conn).unwrap(), AppPrefs::default());
        assert!(VIEWS.contains(&"settings") && !VIEWS.contains(&"ai"));
    }
}
