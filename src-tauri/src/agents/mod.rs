//! Agent Command Center backend. An agent task is a persisted *run* made of *steps*, each owned by
//! one role (see `roles`). One engine (`engine`) drives every run; agents are not free-running
//! processes. All file writes happen in an isolated workspace (`workspace`), every command and every
//! change to the project waits for explicit approval, and every action is recorded as an event.

pub mod engine;
pub mod roles;
pub mod workspace;

use crate::ai::ModelClient;
use crate::error::{AppError, Result};
use crate::projects::project_root;
use crate::{activity, db, AppState};
use roles::Role;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Runtime, State};
use tokio::sync::{mpsc, watch};

pub const EVENT_CHANGED: &str = "nexus://agent-changed";
pub const EVENT_OUTPUT: &str = "nexus://agent-output";

#[derive(Serialize, Clone, Debug)]
pub struct Run {
    pub id: String,
    pub project_id: String,
    pub project_name: Option<String>,
    pub title: String,
    pub outcome: String,
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    /// queued, running, paused, completed, failed, cancelled, rejected, discarded or interrupted.
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    /// Steps currently waiting for the user's decision.
    pub pending_approvals: i64,
    #[serde(skip)]
    pub worktree: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Step {
    pub id: String,
    pub run_id: String,
    pub position: i64,
    /// A role id, or "user" for approval gates.
    pub role: String,
    /// research, plan, plan_approval, develop, discover, command, review or apply.
    pub kind: String,
    pub title: String,
    /// pending, running, awaiting_approval, completed, failed, skipped, rejected or cancelled.
    pub status: String,
    pub detail: String,
    pub output: String,
    pub error: Option<String>,
    pub attempts: i64,
    pub depends_on: Vec<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Event {
    id: i64,
    step_id: Option<String>,
    at: String,
    role: String,
    /// info, model, tool, approval, output or error.
    level: String,
    message: String,
}

#[derive(Serialize)]
pub struct RunDetail {
    run: Run,
    steps: Vec<Step>,
    events: Vec<Event>,
}

pub(crate) struct Handle {
    pub cancel: watch::Sender<bool>,
    pub pause: watch::Sender<bool>,
    pub decisions: mpsc::UnboundedSender<(String, bool)>,
}

#[derive(Default)]
pub struct AgentEngine {
    pub(crate) active: Mutex<HashMap<String, Handle>>,
    /// Replaces the configured provider. Used by tests to script or break the model.
    pub(crate) client: Mutex<Option<Arc<dyn ModelClient>>>,
    /// Base delay between model retries; 0 means the default.
    pub(crate) retry_delay_ms: AtomicU64,
}

impl AgentEngine {
    pub(crate) fn is_active(&self, run_id: &str) -> bool {
        self.active.lock().unwrap_or_else(|e| e.into_inner()).contains_key(run_id)
    }
    pub(crate) fn retry_delay(&self) -> u64 {
        match self.retry_delay_ms.load(Ordering::Relaxed) {
            0 => 1000,
            ms => ms,
        }
    }
}

// ---------------------------------------------------------------- store

const RUN_SELECT: &str = "SELECT r.id, r.project_id, p.name, r.title, r.outcome, r.provider_id, pr.name, r.model, r.status,
    r.error, r.created_at, r.updated_at, r.finished_at,
    (SELECT COUNT(*) FROM agent_steps s WHERE s.run_id = r.id AND s.status = 'awaiting_approval'), r.worktree
    FROM agent_runs r LEFT JOIN projects p ON p.id = r.project_id LEFT JOIN providers pr ON pr.id = r.provider_id";

fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        project_id: r.get(1)?,
        project_name: r.get(2)?,
        title: r.get(3)?,
        outcome: r.get(4)?,
        provider_id: r.get(5)?,
        provider_name: r.get(6)?,
        model: r.get(7)?,
        status: r.get(8)?,
        error: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        finished_at: r.get(12)?,
        pending_approvals: r.get(13)?,
        worktree: r.get(14)?,
    })
}

pub(crate) fn load_run(conn: &Connection, id: &str) -> Result<Run> {
    conn.query_row(&format!("{RUN_SELECT} WHERE r.id = ?1"), [id], row_to_run).optional()?.ok_or_else(|| "Agent task not found".into())
}

const STEP_COLS: &str =
    "id, run_id, position, role, kind, title, status, detail, output, error, attempts, depends_on, started_at, finished_at";

fn row_to_step(r: &rusqlite::Row<'_>) -> rusqlite::Result<Step> {
    let deps: String = r.get(11)?;
    Ok(Step {
        id: r.get(0)?,
        run_id: r.get(1)?,
        position: r.get(2)?,
        role: r.get(3)?,
        kind: r.get(4)?,
        title: r.get(5)?,
        status: r.get(6)?,
        detail: r.get(7)?,
        output: r.get(8)?,
        error: r.get(9)?,
        attempts: r.get(10)?,
        depends_on: serde_json::from_str(&deps).unwrap_or_default(),
        started_at: r.get(12)?,
        finished_at: r.get(13)?,
    })
}

pub(crate) fn load_steps(conn: &Connection, run_id: &str) -> Result<Vec<Step>> {
    let mut stmt = conn.prepare(&format!("SELECT {STEP_COLS} FROM agent_steps WHERE run_id = ?1 ORDER BY position"))?;
    let rows = stmt.query_map([run_id], row_to_step)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[allow(clippy::too_many_arguments)] // one column per argument; a struct would only rename them
pub(crate) fn insert_step(
    conn: &Connection,
    run_id: &str,
    position: i64,
    role: &str,
    kind: &str,
    title: &str,
    detail: &str,
    deps: &[String],
) -> Result<String> {
    let id = db::new_id();
    conn.execute(
        "INSERT INTO agent_steps (id, run_id, position, role, kind, title, status, detail, depends_on)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)",
        params![id, run_id, position, role, kind, title, detail, serde_json::to_string(deps)?],
    )?;
    Ok(id)
}

/// Update a step's status; `output` and `error` are only overwritten when given.
pub(crate) fn set_step(conn: &Connection, id: &str, status: &str, output: Option<&str>, error: Option<&str>) -> Result<()> {
    let now = db::now();
    let finished = matches!(status, "completed" | "failed" | "skipped" | "rejected" | "cancelled").then_some(now.as_str());
    let started = (status == "running").then_some(now.as_str());
    conn.execute(
        "UPDATE agent_steps SET status = ?1, output = COALESCE(?2, output), error = ?3,
         started_at = COALESCE(?4, started_at), finished_at = ?5 WHERE id = ?6",
        params![status, output, error, started, finished, id],
    )?;
    Ok(())
}

pub(crate) fn set_run_status(conn: &Connection, id: &str, status: &str, error: Option<&str>) -> Result<()> {
    let now = db::now();
    let finished = matches!(status, "completed" | "failed" | "cancelled" | "rejected" | "discarded").then_some(now.as_str());
    conn.execute(
        "UPDATE agent_runs SET status = ?1, error = ?2, updated_at = ?3, finished_at = ?4 WHERE id = ?5",
        params![status, error, now, finished, id],
    )?;
    Ok(())
}

/// Append to the run's trace. Never fails the caller.
pub(crate) fn log_event(conn: &Connection, run_id: &str, step_id: Option<&str>, role: &str, level: &str, message: &str) {
    let _ = conn.execute(
        "INSERT INTO agent_events (run_id, step_id, at, role, level, message) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![run_id, step_id, db::now(), role, level, message],
    );
}

fn load_events(conn: &Connection, run_id: &str) -> Result<Vec<Event>> {
    let mut stmt =
        conn.prepare("SELECT id, step_id, at, role, level, message FROM agent_events WHERE run_id = ?1 ORDER BY id DESC LIMIT 800")?;
    let mut events: Vec<Event> = stmt
        .query_map([run_id], |r| {
            Ok(Event { id: r.get(0)?, step_id: r.get(1)?, at: r.get(2)?, role: r.get(3)?, level: r.get(4)?, message: r.get(5)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    events.reverse();
    Ok(events)
}

pub(crate) fn workspace_dir(state: &AppState, run_id: &str) -> PathBuf {
    state.data_dir.join("workspaces").join(run_id)
}

/// Runs that were executing when the app last closed cannot still be running. Mark them so the
/// user can resume them; nothing restarts on its own.
pub fn recover_interrupted(db: &db::Db) -> Result<()> {
    let conn = db.conn();
    conn.execute("UPDATE agent_steps SET status = 'pending', started_at = NULL WHERE status = 'running'", [])?;
    conn.execute(
        "UPDATE agent_runs SET status = 'interrupted', error = 'NEXUS closed while this task was in progress. Resume to continue from the last finished step.', updated_at = ?1
         WHERE status IN ('running', 'queued', 'paused')",
        [db::now()],
    )?;
    Ok(())
}

/// The fixed pipeline every run starts with. Test commands are inserted after discovery.
fn insert_pipeline(conn: &Connection, run_id: &str) -> Result<()> {
    let research = insert_step(conn, run_id, 1, Role::Research.id(), "research", "Gather evidence from the codebase", "", &[])?;
    let plan = insert_step(conn, run_id, 2, Role::Architect.id(), "plan", "Analyse structure and propose a plan", "", &[research])?;
    let gate = insert_step(conn, run_id, 3, "user", "plan_approval", "Approve the plan", "", &[plan])?;
    let develop =
        insert_step(conn, run_id, 4, Role::Developer.id(), "develop", "Implement the plan in an isolated workspace", "", &[gate])?;
    let discover = insert_step(conn, run_id, 5, Role::Testing.id(), "discover", "Discover tests", "", std::slice::from_ref(&develop))?;
    let review = insert_step(conn, run_id, 6, Role::Review.id(), "review", "Review the changes", "", &[develop, discover])?;
    insert_step(conn, run_id, 7, "user", "apply", "Apply the changes to your project", "", &[review])?;
    Ok(())
}

/// File list recovered from a stored patch, for runs whose workspace is already gone.
fn files_from_patch(patch: &str) -> Vec<workspace::ChangedFile> {
    let mut files: Vec<workspace::ChangedFile> = vec![];
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some((_, path)) = rest.split_once(" b/") {
                files.push(workspace::ChangedFile { status: "modified".into(), path: path.to_string() });
            }
        } else if line.starts_with("new file mode") {
            if let Some(f) = files.last_mut() {
                f.status = "added".into();
            }
        } else if line.starts_with("deleted file mode") {
            if let Some(f) = files.last_mut() {
                f.status = "deleted".into();
            }
        }
    }
    files
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn agent_roles() -> Vec<roles::RoleInfo> {
    roles::role_infos()
}

#[tauri::command]
pub fn list_agent_runs(state: State<'_, AppState>) -> Result<Vec<Run>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(&format!("{RUN_SELECT} ORDER BY r.created_at DESC"))?;
    let rows = stmt.query_map([], row_to_run)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn get_agent_run(state: State<'_, AppState>, id: String) -> Result<RunDetail> {
    let conn = state.db.conn();
    Ok(RunDetail { run: load_run(&conn, &id)?, steps: load_steps(&conn, &id)?, events: load_events(&conn, &id)? })
}

#[tauri::command]
pub fn create_agent_run<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    project_id: String,
    outcome: String,
    provider_id: String,
    model: Option<String>,
    allow_remote: Option<bool>,
) -> Result<Run> {
    let outcome = outcome.trim().to_string();
    if outcome.len() < 8 {
        return Err("Describe the outcome you want in a sentence or more.".into());
    }
    project_root(&state, &project_id)?;
    {
        let conn = state.db.conn();
        let limit = crate::exec::load(&conn).max_queued_agents as i64;
        let queued: i64 = conn.query_row("SELECT COUNT(*) FROM agent_runs WHERE status = 'queued'", [], |r| r.get(0))?;
        if queued >= limit {
            return Err(AppError::Msg(format!(
                "{queued} agent tasks are already waiting (Settings → Execution allows {limit}). Let some finish or cancel them first."
            )));
        }
    }
    let id = db::new_id();
    {
        let conn = state.db.conn();
        let scripted = state.agents.client.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let provider: Option<(String, Option<String>, bool, String)> = conn
            .query_row("SELECT name, default_model, enabled, base_url FROM providers WHERE id = ?1", [&provider_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?))
            })
            .optional()?;
        // The task is pinned to one connection and one model when it is created. It never
        // falls back to another model or provider later: if this one stops working, the task
        // fails and can be resumed.
        let mut model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        if !scripted {
            let Some((name, default_model, enabled, base_url)) = provider else {
                return Err("Choose an AI provider. Agents cannot run without one.".into());
            };
            if !enabled {
                return Err(AppError::Msg(format!(
                    "The connection “{name}” is disabled. Enable it in Model Lab → Providers, or choose another."
                )));
            }
            // An agent task sends project code to its model. For a model that is not on this
            // machine the user has to say so when creating the task.
            if !crate::ai::is_local_url(&base_url) && allow_remote != Some(true) {
                return Err(AppError::Msg(format!(
                    "“{name}” is not on this machine: this task would send project code to it. Confirm that when creating the task."
                )));
            }
            model = model.or(default_model.filter(|m| !m.trim().is_empty()));
            if model.is_none() {
                return Err("Choose a model for this task. Agents only use the model you select.".into());
            }
        }
        let title: String = outcome.lines().next().unwrap_or("").chars().take(70).collect();
        let now = db::now();
        conn.execute(
            "INSERT INTO agent_runs (id, project_id, title, outcome, provider_id, model, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?7)",
            params![id, project_id, title, outcome, provider_id, model, now],
        )?;
        insert_pipeline(&conn, &id)?;
        log_event(&conn, &id, None, "engine", "info", "Task created and queued");
        activity::log(&conn, "agent", &format!("Created agent task “{title}”"), Some(&project_id));
    }
    engine::pump(&app);
    let run = load_run(&state.db.conn(), &id)?;
    Ok(run)
}

/// Approve or reject the step that is waiting. This is the only way a plan proceeds, a command
/// runs, or a change reaches the project.
#[tauri::command]
pub fn agent_decide(state: State<'_, AppState>, run_id: String, step_id: String, approved: bool) -> Result<()> {
    let waiting: Option<String> = state
        .db
        .conn()
        .query_row("SELECT status FROM agent_steps WHERE id = ?1 AND run_id = ?2", params![step_id, run_id], |r| r.get(0))
        .optional()?;
    if waiting.as_deref() != Some("awaiting_approval") {
        return Err("This step is not waiting for a decision.".into());
    }
    match state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id) {
        Some(h) => h.decisions.send((step_id, approved)).map_err(|_| "The task is no longer running.".into()),
        None => Err("This task is not running. Resume it first.".into()),
    }
}

#[tauri::command]
pub fn agent_pause(state: State<'_, AppState>, run_id: String) -> Result<()> {
    match state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id) {
        Some(h) => {
            let _ = h.pause.send(true);
            Ok(())
        }
        None => Err("This task is not running.".into()),
    }
}

/// Un-pause a running task, or re-queue one that failed or was interrupted. Finished steps are
/// kept; execution continues from the first unfinished one.
#[tauri::command]
pub fn agent_resume<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, run_id: String) -> Result<()> {
    if let Some(h) = state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id) {
        let _ = h.pause.send(false);
        return Ok(());
    }
    {
        let conn = state.db.conn();
        let run = load_run(&conn, &run_id)?;
        if !matches!(run.status.as_str(), "failed" | "interrupted" | "paused") {
            return Err(AppError::Msg(format!("A {} task cannot be resumed.", run.status)));
        }
        conn.execute(
            "UPDATE agent_steps SET status = 'pending', error = NULL, started_at = NULL, finished_at = NULL
             WHERE run_id = ?1 AND status IN ('failed', 'cancelled', 'running', 'awaiting_approval') AND kind != 'command'",
            [&run_id],
        )?;
        conn.execute("UPDATE agent_steps SET status = 'pending' WHERE run_id = ?1 AND status = 'awaiting_approval'", [&run_id])?;
        set_run_status(&conn, &run_id, "queued", None)?;
        log_event(&conn, &run_id, None, "engine", "info", "Resumed; continuing from the first unfinished step");
    }
    engine::pump(&app);
    Ok(())
}

#[tauri::command]
pub fn agent_cancel<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, run_id: String) -> Result<()> {
    if let Some(h) = state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).get(&run_id) {
        let _ = h.cancel.send(true);
        return Ok(());
    }
    let conn = state.db.conn();
    let run = load_run(&conn, &run_id)?;
    if !matches!(run.status.as_str(), "queued" | "interrupted" | "paused" | "failed") {
        return Err("This task has already finished.".into());
    }
    conn.execute(
        "UPDATE agent_steps SET status = 'cancelled' WHERE run_id = ?1 AND status IN ('pending', 'running', 'awaiting_approval')",
        [&run_id],
    )?;
    set_run_status(&conn, &run_id, "cancelled", None)?;
    log_event(&conn, &run_id, None, "engine", "info", "Cancelled");
    drop(conn);
    engine::discard_workspace(&state, &run_id);
    engine::notify(&app, &run_id);
    Ok(())
}

#[tauri::command]
pub fn delete_agent_run(state: State<'_, AppState>, id: String) -> Result<()> {
    if state.agents.is_active(&id) {
        return Err("Cancel the task before deleting it.".into());
    }
    engine::discard_workspace(&state, &id);
    state.db.conn().execute("DELETE FROM agent_runs WHERE id = ?1", [&id])?;
    Ok(())
}

/// The exact changes a run proposes (or proposed), as a patch plus a file list.
#[tauri::command]
pub async fn agent_diff(state: State<'_, AppState>, run_id: String) -> Result<workspace::Diff> {
    let (worktree, patch): (Option<String>, Option<String>) = state
        .db
        .conn()
        .query_row("SELECT worktree, patch FROM agent_runs WHERE id = ?1", [&run_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or("Agent task not found")?;
    if let Some(dir) = worktree.map(PathBuf::from).filter(|d| d.is_dir()) {
        return workspace::diff(&dir);
    }
    let patch = patch.unwrap_or_default();
    Ok(workspace::Diff { files: files_from_patch(&patch), patch })
}

#[cfg(test)]
mod tests;
