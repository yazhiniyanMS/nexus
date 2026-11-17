//! SQLite persistence. All application state lives in one local database file.

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub struct Db(Mutex<Connection>);

/// One schema step. `down` exactly reverses `up` so a release can be rolled back.
struct Migration {
    up: &'static str,
    /// Not run by the app; kept for manual rollback and exercised by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    down: &'static str,
}

/// Ordered schema migrations. Append only; never edit a shipped entry.
const MIGRATIONS: &[Migration] = &[
    Migration {
        up: r#"
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    added_at TEXT NOT NULL,
    analyzed_at TEXT,
    analysis TEXT
);
CREATE TABLE providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    base_url TEXT NOT NULL,
    default_model TEXT,
    has_key INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE task_steps (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    kind TEXT NOT NULL,
    command TEXT,
    status TEXT NOT NULL,
    output TEXT NOT NULL DEFAULT '',
    exit_code INTEGER,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX idx_steps_task ON task_steps(task_id, position);
CREATE TABLE activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    message TEXT NOT NULL,
    project_id TEXT
);
CREATE TABLE ai_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0
);
"#,
        down: "DROP TABLE ai_usage; DROP TABLE activity; DROP TABLE task_steps; DROP TABLE tasks;
               DROP TABLE providers; DROP TABLE projects;",
    },
    // Code index for Project Intelligence.
    Migration {
        up: r#"
CREATE TABLE intel_files (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    lines INTEGER NOT NULL,
    PRIMARY KEY (project_id, path)
);
CREATE TABLE intel_symbols (
    id INTEGER PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    signature TEXT NOT NULL
);
CREATE INDEX idx_intel_symbols_name ON intel_symbols(project_id, name COLLATE NOCASE);
CREATE INDEX idx_intel_symbols_path ON intel_symbols(project_id, path);
CREATE TABLE intel_imports (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    from_path TEXT NOT NULL,
    target TEXT NOT NULL,
    external INTEGER NOT NULL,
    line INTEGER NOT NULL
);
CREATE INDEX idx_intel_imports_from ON intel_imports(project_id, from_path);
CREATE INDEX idx_intel_imports_target ON intel_imports(project_id, target);
ALTER TABLE projects ADD COLUMN indexed_at TEXT;
"#,
        down: "DROP TABLE intel_imports; DROP TABLE intel_symbols; DROP TABLE intel_files;
               ALTER TABLE projects DROP COLUMN indexed_at;",
    }, // Agent runs: one row per task, its steps, and an append-only event trace.
    Migration {
        up: r#"
CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    outcome TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT,
    status TEXT NOT NULL,
    worktree TEXT,
    base_commit TEXT,
    patch TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT
);
CREATE TABLE agent_steps (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    output TEXT NOT NULL DEFAULT '',
    error TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    depends_on TEXT NOT NULL DEFAULT '[]',
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX idx_agent_steps_run ON agent_steps(run_id, position);
CREATE TABLE agent_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    step_id TEXT,
    at TEXT NOT NULL,
    role TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX idx_agent_events_run ON agent_events(run_id, id);
"#,
        down: "DROP TABLE agent_events; DROP TABLE agent_steps; DROP TABLE agent_runs;",
    },
    Migration {
        up: r#"
CREATE TABLE term_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    shell TEXT NOT NULL,
    cwd TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    exit_code INTEGER,
    pid INTEGER,
    created_at TEXT NOT NULL,
    ended_at TEXT
);
CREATE INDEX idx_term_sessions_project ON term_sessions(project_id, created_at);
CREATE TABLE term_layouts (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    layout TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#,
        down: "DROP TABLE term_layouts; DROP TABLE term_sessions;",
    },
    // Model Lab: settings, model library, downloads, playground sessions, benchmarks.
    Migration {
        up: r#"
ALTER TABLE providers ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE lab_models (
    key TEXT PRIMARY KEY,
    provider_id TEXT,
    model_id TEXT NOT NULL,
    meta TEXT NOT NULL,
    meta_source TEXT NOT NULL DEFAULT '',
    favorite INTEGER NOT NULL DEFAULT 0,
    available INTEGER NOT NULL DEFAULT 1,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL
);
CREATE INDEX idx_lab_models_provider ON lab_models(provider_id);
CREATE TABLE lab_downloads (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    name TEXT NOT NULL,
    provider_id TEXT,
    dest TEXT,
    total_bytes INTEGER,
    received_bytes INTEGER NOT NULL DEFAULT 0,
    sha256 TEXT,
    verified INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT
);
CREATE TABLE lab_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    provider_id TEXT,
    model TEXT,
    system TEXT NOT NULL DEFAULT '',
    params TEXT NOT NULL DEFAULT '{}',
    messages TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE lab_bench_suites (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    config TEXT NOT NULL,
    hardware TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);
CREATE TABLE lab_bench_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    suite_id TEXT NOT NULL REFERENCES lab_bench_suites(id) ON DELETE CASCADE,
    target INTEGER NOT NULL,
    prompt INTEGER NOT NULL,
    iteration INTEGER NOT NULL,
    warmup INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    error TEXT,
    metrics TEXT NOT NULL DEFAULT '{}',
    memory_bytes INTEGER,
    memory_vram_bytes INTEGER,
    output TEXT NOT NULL DEFAULT '',
    at TEXT NOT NULL
);
CREATE INDEX idx_lab_bench_runs_suite ON lab_bench_runs(suite_id, id);
"#,
        down: "DROP TABLE lab_bench_runs; DROP TABLE lab_bench_suites; DROP TABLE lab_sessions; DROP TABLE lab_downloads;
               DROP TABLE lab_models; DROP TABLE settings; ALTER TABLE providers DROP COLUMN enabled;",
    },
];

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, m) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = conn.transaction()?;
            tx.execute_batch(m.up)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        Ok(Db(Mutex::new(conn)))
    }

    /// Lock the connection. Never hold the guard across an `.await`.
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database from the previous version (through the Terminal milestone) with user data in
    /// it is upgraded in place: nothing is lost and existing connections stay enabled.
    #[test]
    fn upgrading_an_existing_database_keeps_user_data() {
        let conn = Connection::open_in_memory().unwrap();
        let previous = MIGRATIONS.len() - 1;
        for m in &MIGRATIONS[..previous] {
            conn.execute_batch(m.up).unwrap();
        }
        conn.pragma_update(None, "user_version", previous).unwrap();
        conn.execute_batch(
            "INSERT INTO projects (id, name, path, added_at) VALUES ('proj', 'My project', '/tmp/my-project', '2026-01-01T00:00:00Z');
             INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at)
                 VALUES ('ollama', 'Ollama', 'ollama', 'http://localhost:11434', 'qwen:1', 0, '2026-01-01T00:00:00Z'),
                        ('claude', 'Anthropic', 'anthropic', 'https://api.anthropic.com', NULL, 1, '2026-01-02T00:00:00Z');
             INSERT INTO ai_usage (at, provider_id, model, input_tokens, output_tokens) VALUES ('2026-01-03T00:00:00Z', 'ollama', 'qwen:1', 10, 20);
             INSERT INTO term_sessions (id, project_id, title, shell, status, created_at) VALUES ('t', 'proj', 'zsh', '/bin/zsh', 'ended', '2026-01-04T00:00:00Z');",
        ).unwrap();

        let db = Db::init(conn).unwrap();
        let conn = db.conn();
        let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len());
        let providers: Vec<(String, String, Option<String>, i64, i64)> = conn
            .prepare("SELECT id, base_url, default_model, has_key, enabled FROM providers ORDER BY created_at")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            providers,
            [
                ("ollama".to_string(), "http://localhost:11434".to_string(), Some("qwen:1".to_string()), 0, 1),
                ("claude".to_string(), "https://api.anthropic.com".to_string(), None, 1, 1),
            ]
        );
        let count = |table: &str| -> i64 { conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap() };
        assert_eq!((count("projects"), count("ai_usage"), count("term_sessions")), (1, 1, 1));
        for table in ["settings", "lab_models", "lab_downloads", "lab_sessions", "lab_bench_suites", "lab_bench_runs"] {
            assert_eq!(count(table), 0, "{table} exists and starts empty");
        }
        // Opening it again changes nothing.
        drop(conn);
        let again: i64 = db.conn().query_row("SELECT COUNT(*) FROM providers", [], |r| r.get(0)).unwrap();
        assert_eq!(again, 2);
    }

    #[test]
    fn migrations_apply() {
        let db = Db::in_memory().unwrap();
        let v: usize = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len());
    }

    fn tables(db: &Db) -> Vec<String> {
        let conn = db.conn();
        let mut stmt =
            conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn every_migration_reverses_cleanly() {
        let db = Db::in_memory().unwrap();
        let full = tables(&db);
        for (i, m) in MIGRATIONS.iter().enumerate().rev() {
            db.conn().execute_batch(m.down).unwrap_or_else(|e| panic!("down {} failed: {e}", i + 1));
        }
        assert!(tables(&db).is_empty(), "down migrations must remove everything");
        for m in MIGRATIONS {
            db.conn().execute_batch(m.up).unwrap();
        }
        assert_eq!(tables(&db), full);
    }
}
