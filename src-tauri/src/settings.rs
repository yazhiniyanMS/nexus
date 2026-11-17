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
}
