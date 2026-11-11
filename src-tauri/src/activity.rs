//! Development activity log and dashboard statistics.

use crate::error::Result;
use crate::AppState;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct ActivityEntry {
    id: i64,
    at: String,
    kind: String,
    message: String,
    project_id: Option<String>,
    project_name: Option<String>,
}

/// Record an activity entry. Failures are swallowed: logging must never break a feature.
pub fn log(conn: &Connection, kind: &str, message: &str, project_id: Option<&str>) {
    let _ = conn.execute(
        "INSERT INTO activity (at, kind, message, project_id) VALUES (?1, ?2, ?3, ?4)",
        params![crate::db::now(), kind, message, project_id],
    );
}

#[tauri::command]
pub fn list_activity(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<ActivityEntry>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(
        "SELECT a.id, a.at, a.kind, a.message, a.project_id, p.name
         FROM activity a LEFT JOIN projects p ON p.id = a.project_id
         ORDER BY a.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit.unwrap_or(200).min(2000)], |r| {
        Ok(ActivityEntry {
            id: r.get(0)?,
            at: r.get(1)?,
            kind: r.get(2)?,
            message: r.get(3)?,
            project_id: r.get(4)?,
            project_name: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn clear_activity(state: State<'_, AppState>) -> Result<()> {
    state.db.conn().execute("DELETE FROM activity", [])?;
    Ok(())
}

#[derive(Serialize)]
pub struct Stats {
    projects: i64,
    analyzed_projects: i64,
    tasks_total: i64,
    tasks_active: i64,
    tasks_completed: i64,
    providers: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[tauri::command]
pub fn get_stats(state: State<'_, AppState>) -> Result<Stats> {
    let conn = state.db.conn();
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    Ok(Stats {
        projects: one("SELECT COUNT(*) FROM projects")?,
        analyzed_projects: one("SELECT COUNT(*) FROM projects WHERE analysis IS NOT NULL")?,
        tasks_total: one("SELECT COUNT(*) FROM tasks")?,
        tasks_active: one("SELECT COUNT(*) FROM tasks WHERE status IN ('running','awaiting_approval')")?,
        tasks_completed: one("SELECT COUNT(*) FROM tasks WHERE status = 'completed'")?,
        providers: one("SELECT COUNT(*) FROM providers")?,
        input_tokens: one("SELECT COALESCE(SUM(input_tokens),0) FROM ai_usage")?,
        output_tokens: one("SELECT COALESCE(SUM(output_tokens),0) FROM ai_usage")?,
    })
}
