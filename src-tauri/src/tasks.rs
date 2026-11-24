//! Task engine: tasks broken into steps, executed one at a time.
//! Every step waits for explicit user approval before it runs, and a run can be cancelled at any point.

use crate::ai::{self, ChatMessage};
use crate::error::{AppError, Result};
use crate::jobs::{self, Exec};
use crate::{activity, db, AppState};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::{mpsc, watch};

pub const EVENT_CHANGED: &str = "nexus://task-changed";
pub const EVENT_OUTPUT: &str = "nexus://task-output";

#[derive(Serialize, Clone)]
pub struct Task {
    id: String,
    project_id: Option<String>,
    project_name: Option<String>,
    title: String,
    description: String,
    status: String,
    created_at: String,
    updated_at: String,
    step_count: i64,
    steps_done: i64,
    /// This task's own time limit per step, in seconds; None uses the default from Settings.
    timeout_secs: Option<i64>,
}

#[derive(Serialize, Clone)]
pub struct Step {
    id: String,
    task_id: String,
    position: i64,
    title: String,
    /// "manual" (the user does it and confirms) or "command" (NEXUS runs it after approval).
    kind: String,
    command: Option<String>,
    status: String,
    output: String,
    exit_code: Option<i64>,
    started_at: Option<String>,
    finished_at: Option<String>,
    /// "high" when the command looks destructive or privileged.
    risk: String,
}

#[derive(Serialize)]
pub struct TaskDetail {
    task: Task,
    steps: Vec<Step>,
}

struct RunHandle {
    cancel: watch::Sender<bool>,
    approvals: mpsc::UnboundedSender<(String, bool)>,
}

#[derive(Default)]
pub struct Engine {
    runs: Mutex<HashMap<String, RunHandle>>,
}

/// Flag commands that delete data, rewrite history, escalate privileges or pipe downloads into a shell.
pub fn risk_of(command: &str) -> &'static str {
    static P: OnceLock<Regex> = OnceLock::new();
    let re = P.get_or_init(|| {
        Regex::new(
            r"(?i)(\brm\s+-[a-z]*[rf]|\brmdir\b|\bdel\s+/|\bformat\s+[a-z]:|\bsudo\b|\bmkfs|\bdd\s+if=|push\s+.*(--force|-f\b)|reset\s+--hard|clean\s+-[a-z]*f|\bdrop\s+(table|database)|(curl|wget)[^|]*\|\s*(ba|z)?sh|\bchmod\s+-R|\bchown\s+-R|>\s*/dev/(?:sd|hd|vd|nvme|disk|rdisk|mmcblk)|\bshutdown\b|\breboot\b|npm\s+publish|cargo\s+publish|Remove-Item)",
        )
        .unwrap()
    });
    if re.is_match(command) {
        "high"
    } else {
        "normal"
    }
}

const TASK_SELECT: &str = "SELECT t.id, t.project_id, p.name, t.title, t.description, t.status, t.created_at, t.updated_at,
    (SELECT COUNT(*) FROM task_steps s WHERE s.task_id = t.id),
    (SELECT COUNT(*) FROM task_steps s WHERE s.task_id = t.id AND s.status IN ('completed','skipped')),
    t.timeout_secs
    FROM tasks t LEFT JOIN projects p ON p.id = t.project_id";

fn row_to_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        project_id: r.get(1)?,
        project_name: r.get(2)?,
        title: r.get(3)?,
        description: r.get(4)?,
        status: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        step_count: r.get(8)?,
        steps_done: r.get(9)?,
        timeout_secs: r.get(10)?,
    })
}

fn row_to_step(r: &rusqlite::Row<'_>) -> rusqlite::Result<Step> {
    let command: Option<String> = r.get(5)?;
    Ok(Step {
        id: r.get(0)?,
        task_id: r.get(1)?,
        position: r.get(2)?,
        title: r.get(3)?,
        kind: r.get(4)?,
        risk: command.as_deref().map(risk_of).unwrap_or("normal").to_string(),
        command,
        status: r.get(6)?,
        output: r.get(7)?,
        exit_code: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
    })
}

const STEP_COLS: &str = "id, task_id, position, title, kind, command, status, output, exit_code, started_at, finished_at";

fn load_task(conn: &Connection, id: &str) -> Result<Task> {
    conn.query_row(&format!("{TASK_SELECT} WHERE t.id = ?1"), [id], row_to_task).optional()?.ok_or_else(|| "Task not found".into())
}

fn load_steps(conn: &Connection, task_id: &str) -> Result<Vec<Step>> {
    let mut stmt = conn.prepare(&format!("SELECT {STEP_COLS} FROM task_steps WHERE task_id = ?1 ORDER BY position"))?;
    let rows = stmt.query_map([task_id], row_to_step)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn set_task_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute("UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3", params![status, db::now(), id])?;
    Ok(())
}

fn insert_step(conn: &Connection, task_id: &str, title: &str, command: Option<&str>) -> Result<()> {
    let command = command.map(str::trim).filter(|c| !c.is_empty());
    let pos: i64 = conn.query_row("SELECT COALESCE(MAX(position), 0) + 1 FROM task_steps WHERE task_id = ?1", [task_id], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO task_steps (id, task_id, position, title, kind, command, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')",
        params![db::new_id(), task_id, pos, title.trim(), if command.is_some() { "command" } else { "manual" }, command],
    )?;
    Ok(())
}

/// Tasks left mid-run when the app last closed can no longer be running; mark them so.
pub fn recover_interrupted(db: &db::Db) -> Result<()> {
    let conn = db.conn();
    conn.execute(
        "UPDATE task_steps SET status = 'cancelled', finished_at = ?1 WHERE status IN ('running', 'awaiting_approval')",
        [db::now()],
    )?;
    conn.execute("UPDATE tasks SET status = 'cancelled' WHERE status IN ('running', 'awaiting_approval')", [])?;
    Ok(())
}

// ---------------------------------------------------------------- CRUD commands

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> Result<Vec<Task>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(&format!("{TASK_SELECT} ORDER BY t.updated_at DESC"))?;
    let rows = stmt.query_map([], row_to_task)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, id: String) -> Result<TaskDetail> {
    let conn = state.db.conn();
    Ok(TaskDetail { task: load_task(&conn, &id)?, steps: load_steps(&conn, &id)? })
}

#[tauri::command]
pub fn create_task(state: State<'_, AppState>, title: String, description: String, project_id: Option<String>) -> Result<Task> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title".into());
    }
    let id = db::new_id();
    let now = db::now();
    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, description, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
        params![id, project_id, title, description.trim(), now],
    )?;
    activity::log(&conn, "task", &format!("Created task “{title}”"), project_id.as_deref());
    load_task(&conn, &id)
}

fn ensure_idle(state: &AppState, task_id: &str) -> Result<()> {
    if state.engine.runs.lock().unwrap_or_else(|e| e.into_inner()).contains_key(task_id) {
        return Err("Cancel the running task first".into());
    }
    Ok(())
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> Result<()> {
    ensure_idle(&state, &id)?;
    state.db.conn().execute("DELETE FROM tasks WHERE id = ?1", [&id])?;
    Ok(())
}

#[tauri::command]
pub fn add_step(state: State<'_, AppState>, task_id: String, title: String, command: Option<String>) -> Result<()> {
    if title.trim().is_empty() {
        return Err("A step needs a title".into());
    }
    let conn = state.db.conn();
    load_task(&conn, &task_id)?;
    insert_step(&conn, &task_id, &title, command.as_deref())
}

#[tauri::command]
pub fn delete_step(state: State<'_, AppState>, task_id: String, step_id: String) -> Result<()> {
    ensure_idle(&state, &task_id)?;
    state.db.conn().execute("DELETE FROM task_steps WHERE id = ?1 AND task_id = ?2", params![step_id, task_id])?;
    Ok(())
}

#[tauri::command]
pub fn reset_task(state: State<'_, AppState>, id: String) -> Result<()> {
    ensure_idle(&state, &id)?;
    let conn = state.db.conn();
    conn.execute(
        "UPDATE task_steps SET status = 'pending', output = '', exit_code = NULL, started_at = NULL, finished_at = NULL
         WHERE task_id = ?1",
        [&id],
    )?;
    set_task_status(&conn, &id, "pending")
}

// ---------------------------------------------------------------- step generation

/// Where a task's commands run: its project folder, or the home folder for a task without a
/// project. A task whose project folder is gone gets an error instead of running somewhere else.
fn command_dir(conn: &Connection, task: &Task) -> std::result::Result<PathBuf, String> {
    match &task.project_id {
        Some(_) => project_dir(conn, task)
            .ok_or_else(|| "The project folder for this task no longer exists, so the command was not run.".to_string()),
        None => crate::platform::paths::home().filter(|h| h.is_dir()).ok_or_else(|| "No home folder to run the command in.".to_string()),
    }
}

fn project_dir(conn: &Connection, task: &Task) -> Option<PathBuf> {
    let id = task.project_id.as_ref()?;
    let path: String = conn.query_row("SELECT path FROM projects WHERE id = ?1", [id], |r| r.get(0)).ok()?;
    Some(PathBuf::from(path)).filter(|p| p.is_dir())
}

/// Derive a sensible verification workflow from the project's manifests. No AI involved.
pub fn suggested_for(dir: &Path) -> Vec<(String, Option<String>)> {
    let mut steps: Vec<(String, Option<String>)> = vec![];
    let has = |f: &str| dir.join(f).exists();
    let cmd = |steps: &mut Vec<(String, Option<String>)>, title: &str, c: String| steps.push((title.to_string(), Some(c)));
    if has(".git") {
        cmd(&mut steps, "Check the working tree", "git status --short --branch".into());
    }
    if has("package.json") {
        let pm = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "yarn"
        } else if has("bun.lockb") {
            "bun"
        } else {
            "npm"
        };
        let scripts: Vec<String> = std::fs::read_to_string(dir.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("scripts").and_then(|s| s.as_object()).map(|m| m.keys().cloned().collect()))
            .unwrap_or_default();
        cmd(&mut steps, "Install dependencies", format!("{pm} install"));
        for s in ["lint", "typecheck", "test", "build"] {
            if scripts.iter().any(|x| x == s) {
                cmd(&mut steps, &format!("Run {s}"), format!("{pm} run {s}"));
            }
        }
    }
    let cargo_dir = if has("Cargo.toml") {
        Some("")
    } else if has("src-tauri/Cargo.toml") {
        Some("src-tauri/")
    } else {
        None
    };
    if let Some(prefix) = cargo_dir {
        let flag = if prefix.is_empty() { String::new() } else { format!(" --manifest-path {prefix}Cargo.toml") };
        cmd(&mut steps, "Type-check Rust code", format!("cargo check{flag}"));
        cmd(&mut steps, "Run Rust tests", format!("cargo test{flag}"));
    }
    if has("go.mod") {
        cmd(&mut steps, "Vet Go code", "go vet ./...".into());
        cmd(&mut steps, "Run Go tests", "go test ./...".into());
    }
    if has("pyproject.toml") || has("requirements.txt") {
        cmd(&mut steps, "Run Python tests", "python -m pytest".into());
    }
    if has("Makefile") && steps.len() <= 1 {
        cmd(&mut steps, "Build with make", "make".into());
    }
    steps.push(("Review the results and summarise what changed".into(), None));
    steps
}

#[tauri::command]
pub fn suggest_steps(state: State<'_, AppState>, task_id: String) -> Result<usize> {
    let conn = state.db.conn();
    let task = load_task(&conn, &task_id)?;
    let dir = project_dir(&conn, &task).ok_or("Link this task to a project to get suggested steps")?;
    let steps = suggested_for(&dir);
    for (title, command) in &steps {
        insert_step(&conn, &task_id, title, command.as_deref())?;
    }
    Ok(steps.len())
}

#[derive(Deserialize)]
struct PlannedStep {
    title: String,
    command: Option<String>,
}

/// Pull the first JSON array out of a model reply, tolerating prose or code fences around it.
fn parse_plan(reply: &str) -> Result<Vec<PlannedStep>> {
    let start = reply.find('[').ok_or("The model did not return a step list")?;
    let end = reply.rfind(']').ok_or("The model did not return a step list")?;
    if end < start {
        return Err("The model did not return a step list".into());
    }
    serde_json::from_str::<Vec<PlannedStep>>(&reply[start..=end])
        .map_err(|e| AppError::Msg(format!("Could not read the model's plan: {e}")))
}

#[tauri::command]
pub async fn plan_with_ai(state: State<'_, AppState>, task_id: String, provider_id: String, model: Option<String>) -> Result<usize> {
    let (task, context) = {
        let conn = state.db.conn();
        let task = load_task(&conn, &task_id)?;
        let summary: Option<String> = task.project_id.as_ref().and_then(|pid| {
            conn.query_row("SELECT analysis FROM projects WHERE id = ?1", [pid], |r| r.get::<_, Option<String>>(0))
                .ok()
                .flatten()
                .and_then(|a| serde_json::from_str::<serde_json::Value>(&a).ok())
                .and_then(|v| v.get("summary").and_then(|s| s.as_str()).map(String::from))
        });
        (task, summary.unwrap_or_else(|| "No project analysis available.".into()))
    };
    let os = std::env::consts::OS;
    let system = format!(
        "You break software development tasks into small, concrete steps. Reply with ONLY a JSON array. \
         Each element is {{\"title\": string, \"command\": string or null}}. Use \"command\" only for a single \
         non-interactive shell command valid on {os}, run from the project root; use null for work a human does. \
         Prefer 3 to 8 steps. Never include destructive commands."
    );
    let prompt = format!("Project: {context}\n\nTask: {}\n\nDetails: {}", task.title, task.description);
    let reply = ai::complete(&state, &provider_id, model, Some(system), vec![ChatMessage { role: "user".into(), content: prompt }]).await?;
    let plan = parse_plan(&reply.content)?;
    let conn = state.db.conn();
    for s in &plan {
        insert_step(&conn, &task_id, &s.title, s.command.as_deref())?;
    }
    activity::log(&conn, "ai", &format!("AI planned {} steps for “{}”", plan.len(), task.title), task.project_id.as_deref());
    Ok(plan.len())
}

// ---------------------------------------------------------------- execution

#[tauri::command]
pub fn run_task<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, id: String) -> Result<()> {
    {
        let conn = state.db.conn();
        load_task(&conn, &id)?;
        let pending: i64 =
            conn.query_row("SELECT COUNT(*) FROM task_steps WHERE task_id = ?1 AND status = 'pending'", [&id], |r| r.get(0))?;
        if pending == 0 {
            return Err("No pending steps. Add steps or reset the task.".into());
        }
    }
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (approve_tx, approve_rx) = mpsc::unbounded_channel();
    {
        let mut runs = state.engine.runs.lock().unwrap_or_else(|e| e.into_inner());
        if runs.contains_key(&id) {
            return Err("This task is already running".into());
        }
        let limit = crate::exec::load(&state.db.conn()).max_running_tasks as usize;
        if runs.len() >= limit {
            return Err(AppError::Msg(format!(
                "{limit} tasks are already running (Settings → Execution). Wait for one to finish or cancel it."
            )));
        }
        runs.insert(id.clone(), RunHandle { cancel: cancel_tx, approvals: approve_tx });
    }
    tauri::async_runtime::spawn(async move {
        let outcome = run_loop(&app, &id, cancel_rx, approve_rx).await;
        let state = app.state::<AppState>();
        state.engine.runs.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        let status = match outcome {
            Ok(s) => s,
            Err(e) => {
                activity::log(&state.db.conn(), "task", &format!("Task run error: {e}"), None);
                "failed"
            }
        };
        {
            let conn = state.db.conn();
            let _ = set_task_status(&conn, &id, status);
            if let Ok(task) = load_task(&conn, &id) {
                activity::log(&conn, "task", &format!("Task “{}” {status}", task.title), task.project_id.as_deref());
            }
        }
        let _ = app.emit(EVENT_CHANGED, &id);
    });
    Ok(())
}

/// Set a task's own time limit per step (seconds), or clear it to use the default.
#[tauri::command]
pub fn set_task_timeout(state: State<'_, AppState>, id: String, timeout_secs: Option<u64>) -> Result<Task> {
    if let Some(s) = timeout_secs {
        if !(crate::exec::MIN_TIMEOUT_SECS..=crate::exec::MAX_TIMEOUT_SECS).contains(&s) {
            return Err("A time limit must be between 5 seconds and 24 hours".into());
        }
    }
    let conn = state.db.conn();
    load_task(&conn, &id)?;
    conn.execute(
        "UPDATE tasks SET timeout_secs = ?1, updated_at = ?2 WHERE id = ?3",
        params![timeout_secs.map(|s| s as i64), db::now(), id],
    )?;
    load_task(&conn, &id)
}

#[tauri::command]
pub fn cancel_task(state: State<'_, AppState>, id: String) -> Result<()> {
    match state.engine.runs.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        Some(h) => {
            let _ = h.cancel.send(true);
            Ok(())
        }
        None => Err("This task is not running".into()),
    }
}

#[tauri::command]
pub fn approve_step(state: State<'_, AppState>, task_id: String, step_id: String, approved: bool) -> Result<()> {
    // Only the step the run is waiting on can be decided; an early or repeated decision must
    // not be kept and applied to a later step.
    let waiting: Option<String> = state
        .db
        .conn()
        .query_row("SELECT status FROM task_steps WHERE id = ?1 AND task_id = ?2", params![step_id, task_id], |r| r.get(0))
        .optional()?;
    if waiting.as_deref() != Some("awaiting_approval") {
        return Err("This step is not waiting for a decision.".into());
    }
    match state.engine.runs.lock().unwrap_or_else(|e| e.into_inner()).get(&task_id) {
        Some(h) => h.approvals.send((step_id, approved)).map_err(|_| "The task is no longer running".into()),
        None => Err("This task is not running".into()),
    }
}

fn finish_step(conn: &Connection, id: &str, status: &str, output: Option<&str>, exit: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE task_steps SET status = ?1, output = COALESCE(?2, output), exit_code = ?3, finished_at = ?4 WHERE id = ?5",
        params![status, output, exit, db::now(), id],
    )?;
    Ok(())
}

/// Drive a task to completion. Returns the final task status.
async fn run_loop<R: Runtime>(
    app: &AppHandle<R>,
    task_id: &str,
    mut cancel: watch::Receiver<bool>,
    mut approvals: mpsc::UnboundedReceiver<(String, bool)>,
) -> Result<&'static str> {
    let state = app.state::<AppState>();
    let changed = || {
        let _ = app.emit(EVENT_CHANGED, task_id);
    };
    loop {
        // Next pending step, flagged as waiting for the user.
        let (step, cwd) = {
            let conn = state.db.conn();
            let next = conn
                .query_row(
                    &format!("SELECT {STEP_COLS} FROM task_steps WHERE task_id = ?1 AND status = 'pending' ORDER BY position LIMIT 1"),
                    [task_id],
                    row_to_step,
                )
                .optional()?;
            let Some(step) = next else { return Ok("completed") };
            conn.execute("UPDATE task_steps SET status = 'awaiting_approval' WHERE id = ?1", [&step.id])?;
            set_task_status(&conn, task_id, "awaiting_approval")?;
            let cwd = command_dir(&conn, &load_task(&conn, task_id)?);
            (step, cwd)
        };
        // Only after the database lock is released: the notification asks the main thread
        // whether the window is focused and waits for it, while the main thread may be waiting
        // for the database (a UI refresh). Holding the lock here deadlocked the app.
        crate::platform::notify::attention(app, "NEXUS: a task step needs your approval", &step.title);
        changed();

        // Nothing runs without an explicit decision from the user.
        let approved = loop {
            tokio::select! {
                _ = cancel.changed() => break None,
                msg = approvals.recv() => match msg {
                    Some((id, ok)) if id == step.id => break Some(ok),
                    Some(_) => continue,
                    None => break None,
                },
            }
        };
        let Some(approved) = approved else {
            finish_step(&state.db.conn(), &step.id, "cancelled", None, None)?;
            return Ok("cancelled");
        };
        if !approved {
            finish_step(&state.db.conn(), &step.id, "skipped", None, None)?;
            continue;
        }
        let Some(command) = step.command.as_deref().filter(|_| step.kind == "command") else {
            finish_step(&state.db.conn(), &step.id, "completed", None, None)?;
            continue;
        };

        {
            let conn = state.db.conn();
            conn.execute(
                "UPDATE task_steps SET status = 'running', started_at = ?1, output = '' WHERE id = ?2",
                params![db::now(), step.id],
            )?;
            set_task_status(&conn, task_id, "running")?;
            activity::log(&conn, "command", &format!("Ran `{command}`"), None);
        }
        changed();

        let cwd = match cwd {
            Ok(dir) => dir,
            Err(why) => {
                finish_step(&state.db.conn(), &step.id, "failed", Some(&why), None)?;
                return Ok("failed");
            }
        };
        let policy = {
            let conn = state.db.conn();
            let task = load_task(&conn, task_id)?;
            crate::exec::Policy::for_task(&crate::exec::load(&conn), task.timeout_secs.map(|s| s as u64), Some(&state.data_dir))
        };
        let result = execute(app, task_id, &step.id, command, &cwd, &policy, &mut cancel).await;
        let conn = state.db.conn();
        match result {
            Ok(Exec { output, code: Some(0), cancelled: false, timed_out: false }) => {
                finish_step(&conn, &step.id, "completed", Some(&output), Some(0))?
            }
            Ok(Exec { output, cancelled: true, .. }) => {
                finish_step(&conn, &step.id, "cancelled", Some(&output), None)?;
                return Ok("cancelled");
            }
            Ok(Exec { output, code, .. }) => {
                finish_step(&conn, &step.id, "failed", Some(&output), code)?;
                return Ok("failed");
            }
            Err(e) => {
                finish_step(&conn, &step.id, "failed", Some(&format!("Could not start the command: {e}")), None)?;
                return Ok("failed");
            }
        }
    }
}

#[derive(Serialize, Clone)]
struct OutputEvent<'a> {
    task_id: &'a str,
    step_id: &'a str,
    chunk: &'a str,
}

/// Run one approved step command through the shared job runner, streaming output to the window.
async fn execute<R: Runtime>(
    app: &AppHandle<R>,
    task_id: &str,
    step_id: &str,
    command: &str,
    cwd: &Path,
    policy: &crate::exec::Policy,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Exec> {
    jobs::run_command(&jobs::RunSpec { command, cwd, policy }, cancel, |chunk| {
        let _ = app.emit(EVENT_OUTPUT, OutputEvent { task_id, step_id, chunk });
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_risky_commands() {
        for c in [
            "rm -rf build",
            "sudo apt install x",
            "git push origin main --force",
            "git reset --hard HEAD~1",
            "curl https://x.sh | sh",
            "git clean -fd",
            "npm publish",
            "cat image.iso > /dev/disk4",
            "echo x >/dev/sda",
        ] {
            assert_eq!(risk_of(c), "high", "{c}");
        }
        for c in ["npm run build", "cargo test", "git status", "ls -la", "npm install", "make >/dev/null 2>&1", "ls 2> /dev/stderr"] {
            assert_eq!(risk_of(c), "normal", "{c}");
        }
    }

    #[test]
    fn parses_ai_plan_with_surrounding_text() {
        let plan = parse_plan(
            "Sure!\n```json\n[{\"title\":\"Build\",\"command\":\"npm run build\"},{\"title\":\"Review\",\"command\":null}]\n```",
        )
        .unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].command.as_deref(), Some("npm run build"));
        assert!(parse_plan("no list here").is_err());
    }

    #[test]
    fn suggests_steps_from_manifests() {
        let dir = crate::projects::tests::temp_project(&[
            ("package.json", r#"{"scripts":{"build":"vite build","test":"vitest"}}"#),
            ("pnpm-lock.yaml", ""),
        ]);
        let cmds: Vec<String> = suggested_for(&dir).into_iter().filter_map(|s| s.1).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(cmds, ["pnpm install", "pnpm run test", "pnpm run build"]);
    }

    #[test]
    fn steps_are_ordered_and_recovered() {
        let db = db::Db::in_memory().unwrap();
        {
            let conn = db.conn();
            conn.execute("INSERT INTO tasks (id, title, status, created_at, updated_at) VALUES ('t', 'x', 'running', '', '')", []).unwrap();
            insert_step(&conn, "t", "one", Some("echo 1")).unwrap();
            insert_step(&conn, "t", "two", None).unwrap();
            conn.execute("UPDATE task_steps SET status = 'running' WHERE position = 1", []).unwrap();
        }
        recover_interrupted(&db).unwrap();
        let conn = db.conn();
        let steps = load_steps(&conn, "t").unwrap();
        assert_eq!(steps.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), ["command", "manual"]);
        assert_eq!(steps[0].status, "cancelled");
        assert_eq!(steps[1].status, "pending");
        assert_eq!(load_task(&conn, "t").unwrap().status, "cancelled");
    }
    /// End-to-end run against a mock Tauri app: approval gate, real command execution, skip and cancel.
    #[test]
    fn runs_approves_skips_and_cancels() {
        use std::time::Duration;
        let app = tauri::test::mock_builder().build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(AppState::for_tests());
        let handle = app.handle().clone();
        let state = || handle.state::<AppState>();
        let task = create_task(state(), "e2e".into(), String::new(), None).unwrap();
        for (title, cmd) in [
            ("echo", Some("echo hello-nexus && echo err 1>&2")),
            ("skipped", Some("echo never")),
            ("manual", None),
            // A long command, then cancelled: a background job and a foreground one on Unix.
            ("slow", Some(if cfg!(windows) { "ping -n 30 127.0.0.1 >nul" } else { "sleep 30 & sleep 30" })),
        ] {
            add_step(state(), task.id.clone(), title.into(), cmd.map(String::from)).unwrap();
        }
        let wait_for = |step: usize, status: &str| {
            for _ in 0..200 {
                let d = get_task(state(), task.id.clone()).unwrap();
                if d.steps[step].status == status {
                    return d;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("step {step} never reached {status}");
        };

        run_task(handle.clone(), state(), task.id.clone()).unwrap();
        assert!(run_task(handle.clone(), state(), task.id.clone()).is_err(), "double run must be refused");

        // Nothing executes before approval.
        let d = wait_for(0, "awaiting_approval");
        assert_eq!(d.task.status, "awaiting_approval");
        assert_eq!(d.steps[0].output, "");
        approve_step(state(), task.id.clone(), d.steps[0].id.clone(), true).unwrap();
        let d = wait_for(0, "completed");
        assert!(d.steps[0].output.contains("hello-nexus") && d.steps[0].output.contains("err"));
        assert_eq!(d.steps[0].exit_code, Some(0));

        let d = wait_for(1, "awaiting_approval");
        approve_step(state(), task.id.clone(), d.steps[1].id.clone(), false).unwrap();
        let d = wait_for(1, "skipped");
        assert_eq!(d.steps[1].output, "");

        let d = wait_for(2, "awaiting_approval");
        approve_step(state(), task.id.clone(), d.steps[2].id.clone(), true).unwrap();
        wait_for(2, "completed");

        let d = wait_for(3, "awaiting_approval");
        approve_step(state(), task.id.clone(), d.steps[3].id.clone(), true).unwrap();
        wait_for(3, "running");
        let started = std::time::Instant::now();
        cancel_task(state(), task.id.clone()).unwrap();
        let d = wait_for(3, "cancelled");
        assert!(started.elapsed() < Duration::from_secs(5), "cancel must not wait for the command");
        assert!(d.steps[3].output.contains("[cancelled]"));
        for _ in 0..200 {
            if get_task(state(), task.id.clone()).unwrap().task.status == "cancelled" {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(get_task(state(), task.id.clone()).unwrap().task.status, "cancelled");
        assert!(cancel_task(state(), task.id.clone()).is_err(), "run handle must be released");
    }

    /// A task whose project folder disappeared must not run its command somewhere else, and a
    /// step that is not waiting cannot be approved ahead of time.
    #[test]
    fn a_missing_project_folder_stops_the_command_and_early_approvals_are_refused() {
        use std::time::Duration;
        let app = tauri::test::mock_builder().build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(AppState::for_tests());
        let handle = app.handle().clone();
        let state = || handle.state::<AppState>();
        let project = crate::projects::tests::temp_project(&[("a.txt", "x")]);
        state()
            .db
            .conn()
            .execute("INSERT INTO projects (id, name, path, added_at) VALUES ('p', 'P', ?1, 'x')", [project.to_string_lossy()])
            .unwrap();
        let marker = std::env::temp_dir().join(format!("nexus-ran-{}", db::new_id()));
        let task = create_task(state(), "moved".into(), String::new(), Some("p".into())).unwrap();
        add_step(state(), task.id.clone(), "touch".into(), Some(format!("touch \"{}\"", marker.display()))).unwrap();
        add_step(state(), task.id.clone(), "second".into(), Some("echo two".into())).unwrap();
        std::fs::remove_dir_all(&project).unwrap();

        run_task(handle.clone(), state(), task.id.clone()).unwrap();
        let mut d = get_task(state(), task.id.clone()).unwrap();
        for _ in 0..200 {
            d = get_task(state(), task.id.clone()).unwrap();
            if d.steps[0].status == "awaiting_approval" {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let err = approve_step(state(), task.id.clone(), d.steps[1].id.clone(), true).unwrap_err().to_string();
        assert!(err.contains("not waiting"), "{err}");
        approve_step(state(), task.id.clone(), d.steps[0].id.clone(), true).unwrap();
        for _ in 0..200 {
            d = get_task(state(), task.id.clone()).unwrap();
            if d.task.status == "failed" {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(d.task.status, "failed");
        assert_eq!(d.steps[0].status, "failed");
        assert!(d.steps[0].output.contains("no longer exists"), "{}", d.steps[0].output);
        assert_eq!(d.steps[1].status, "pending", "later steps do not run");
        assert!(!marker.exists(), "the command ran somewhere else");
    }

    /// A task's own time limit applies to its steps: a hanging step is stopped, recorded as
    /// failed with the reason, and the task fails without running later steps.
    #[cfg(unix)]
    #[test]
    fn a_task_time_limit_stops_a_hanging_step() {
        use std::time::{Duration, Instant};
        let app = tauri::test::mock_builder().build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(AppState::for_tests());
        let handle = app.handle().clone();
        let state = || handle.state::<AppState>();
        let task = create_task(state(), "slow".into(), String::new(), None).unwrap();
        assert!(set_task_timeout(state(), task.id.clone(), Some(1)).is_err(), "below the minimum");
        assert!(set_task_timeout(state(), task.id.clone(), Some(crate::exec::MAX_TIMEOUT_SECS + 1)).is_err());
        let t = set_task_timeout(state(), task.id.clone(), Some(crate::exec::MIN_TIMEOUT_SECS)).unwrap();
        assert_eq!(t.timeout_secs, Some(crate::exec::MIN_TIMEOUT_SECS as i64));
        add_step(state(), task.id.clone(), "hang".into(), Some("sleep 120".into())).unwrap();
        add_step(state(), task.id.clone(), "after".into(), Some("echo after".into())).unwrap();
        run_task(handle.clone(), state(), task.id.clone()).unwrap();
        let wait = |pred: &dyn Fn(&TaskDetail) -> bool| {
            for _ in 0..600 {
                let d = get_task(state(), task.id.clone()).unwrap();
                if pred(&d) {
                    return d;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("never happened");
        };
        let d = wait(&|d| d.steps[0].status == "awaiting_approval");
        approve_step(state(), task.id.clone(), d.steps[0].id.clone(), true).unwrap();
        let started = Instant::now();
        let d = wait(&|d| d.task.status == "failed");
        assert!(started.elapsed() < Duration::from_secs(crate::exec::MIN_TIMEOUT_SECS + 6), "{:?}", started.elapsed());
        assert_eq!(d.steps[0].status, "failed");
        assert_eq!(d.steps[0].exit_code, None);
        assert!(d.steps[0].output.contains("ran longer than its time limit of 5 s"), "{}", d.steps[0].output);
        assert_eq!(d.steps[1].status, "pending");
        assert!(set_task_timeout(state(), task.id.clone(), None).unwrap().timeout_secs.is_none(), "cleared back to the default");
    }

    #[test]
    fn failing_command_fails_the_task() {
        use std::time::Duration;
        let app = tauri::test::mock_builder().build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        app.manage(AppState::for_tests());
        let handle = app.handle().clone();
        let state = || handle.state::<AppState>();
        let task = create_task(state(), "fail".into(), String::new(), None).unwrap();
        add_step(
            state(),
            task.id.clone(),
            "bad".into(),
            Some(if cfg!(windows) { "echo boom & exit 3" } else { "echo boom; exit 3" }.into()),
        )
        .unwrap();
        add_step(state(), task.id.clone(), "after".into(), Some("echo no".into())).unwrap();
        run_task(handle.clone(), state(), task.id.clone()).unwrap();
        let mut approved = false;
        for _ in 0..300 {
            let d = get_task(state(), task.id.clone()).unwrap();
            if !approved && d.steps[0].status == "awaiting_approval" {
                approve_step(state(), task.id.clone(), d.steps[0].id.clone(), true).unwrap();
                approved = true;
            }
            if d.task.status == "failed" {
                assert_eq!(d.steps[0].exit_code, Some(3));
                assert!(d.steps[0].output.contains("boom"));
                assert_eq!(d.steps[1].status, "pending", "later steps must not run after a failure");
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("task never failed");
    }
}
