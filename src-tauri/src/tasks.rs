//! Task engine: tasks broken into steps, executed one at a time.
//! Every step waits for explicit user approval before it runs, and a run can be cancelled at any point.

use crate::ai::{self, ChatMessage};
use crate::error::{AppError, Result};
use crate::{activity, db, AppState};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{mpsc, watch};

const MAX_OUTPUT_BYTES: usize = 200_000;
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
            r"(?i)(\brm\s+-[a-z]*[rf]|\brmdir\b|\bdel\s+/|\bformat\s+[a-z]:|\bsudo\b|\bmkfs|\bdd\s+if=|push\s+.*(--force|-f\b)|reset\s+--hard|clean\s+-[a-z]*f|\bdrop\s+(table|database)|(curl|wget)[^|]*\|\s*(ba|z)?sh|\bchmod\s+-R|\bchown\s+-R|>\s*/dev/|\bshutdown\b|\breboot\b|npm\s+publish|cargo\s+publish|Remove-Item)",
        )
        .unwrap()
    });
    if re.is_match(command) { "high" } else { "normal" }
}

const TASK_SELECT: &str = "SELECT t.id, t.project_id, p.name, t.title, t.description, t.status, t.created_at, t.updated_at,
    (SELECT COUNT(*) FROM task_steps s WHERE s.task_id = t.id),
    (SELECT COUNT(*) FROM task_steps s WHERE s.task_id = t.id AND s.status IN ('completed','skipped'))
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
    conn.query_row(&format!("{TASK_SELECT} WHERE t.id = ?1"), [id], row_to_task)
        .optional()?
        .ok_or_else(|| "Task not found".into())
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
    let pos: i64 =
        conn.query_row("SELECT COALESCE(MAX(position), 0) + 1 FROM task_steps WHERE task_id = ?1", [task_id], |r| r.get(0))?;
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
pub fn create_task(
    state: State<'_, AppState>,
    title: String,
    description: String,
    project_id: Option<String>,
) -> Result<Task> {
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
        let pm = if has("pnpm-lock.yaml") { "pnpm" } else if has("yarn.lock") { "yarn" } else if has("bun.lockb") { "bun" } else { "npm" };
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
    let cargo_dir = if has("Cargo.toml") { Some("") } else if has("src-tauri/Cargo.toml") { Some("src-tauri/") } else { None };
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
pub async fn plan_with_ai(
    state: State<'_, AppState>,
    task_id: String,
    provider_id: String,
    model: Option<String>,
) -> Result<usize> {
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
    let reply = ai::complete(&state, &provider_id, model, Some(system), vec![ChatMessage { role: "user".into(), content: prompt }])
        .await?;
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
pub fn run_task(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    {
        let conn = state.db.conn();
        load_task(&conn, &id)?;
        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_steps WHERE task_id = ?1 AND status = 'pending'",
            [&id],
            |r| r.get(0),
        )?;
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
async fn run_loop(
    app: &AppHandle,
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
            let cwd = project_dir(&conn, &load_task(&conn, task_id)?);
            (step, cwd)
        };
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

        let result = execute(app, task_id, &step.id, command, cwd.as_deref(), &mut cancel).await;
        let conn = state.db.conn();
        match result {
            Ok(Exec { output, code: Some(0), cancelled: false }) => finish_step(&conn, &step.id, "completed", Some(&output), Some(0))?,
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

struct Exec {
    output: String,
    code: Option<i64>,
    cancelled: bool,
}

fn spawn_reader<R: AsyncRead + Unpin + Send + 'static>(stream: R, tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

#[derive(Serialize, Clone)]
struct OutputEvent<'a> {
    task_id: &'a str,
    step_id: &'a str,
    chunk: &'a str,
}

async fn execute(
    app: &AppHandle,
    task_id: &str,
    step_id: &str,
    command: &str,
    cwd: Option<&Path>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Exec> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]).process_group(0);
        c
    };
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, tx.clone());
    }
    drop(tx);

    let mut output = String::new();
    let mut cancelled = false;
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(line) => {
                    if output.len() < MAX_OUTPUT_BYTES {
                        output.push_str(&line);
                        let _ = app.emit(EVENT_OUTPUT, OutputEvent { task_id, step_id, chunk: &line });
                        if output.len() >= MAX_OUTPUT_BYTES {
                            output.push_str("\n… output truncated …\n");
                        }
                    }
                }
                None => break,
            },
            _ = cancel.changed() => {
                cancelled = true;
                kill_tree(pid);
                let _ = child.start_kill();
                break;
            }
        }
    }
    let status = child.wait().await?;
    if cancelled {
        output.push_str("\n[cancelled]\n");
    }
    Ok(Exec { output, code: status.code().map(i64::from), cancelled })
}

/// Kill the command and everything it spawned, not just the wrapping shell.
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        // The child was started as its own process-group leader, so this reaches its descendants.
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_risky_commands() {
        for c in ["rm -rf build", "sudo apt install x", "git push origin main --force", "git reset --hard HEAD~1",
                  "curl https://x.sh | sh", "git clean -fd", "npm publish"] {
            assert_eq!(risk_of(c), "high", "{c}");
        }
        for c in ["npm run build", "cargo test", "git status", "ls -la", "npm install"] {
            assert_eq!(risk_of(c), "normal", "{c}");
        }
    }

    #[test]
    fn parses_ai_plan_with_surrounding_text() {
        let plan = parse_plan("Sure!\n```json\n[{\"title\":\"Build\",\"command\":\"npm run build\"},{\"title\":\"Review\",\"command\":null}]\n```").unwrap();
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
}
