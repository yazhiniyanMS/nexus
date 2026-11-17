//! The orchestration engine: a queue, a dependency-ordered step loop, approval gates, model
//! retries, pause, cancellation and a persisted trace. State is written to SQLite before anything
//! else happens, so a run can always be resumed from its last finished step.

use super::roles::{self, Capability, Changes, Plan, ReviewReport, Role};
use super::{
    insert_step, load_run, load_steps, log_event, set_run_status, set_step, workspace, workspace_dir, Handle, Step, EVENT_CHANGED,
    EVENT_OUTPUT,
};
use crate::ai::{ModelClient, ProviderClient};
use crate::error::AppError;
use crate::projects::project_root;
use crate::{activity, intel, jobs, tasks, AppState};
use rusqlite::params;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::{mpsc, watch};

/// Runs executing at once. Others wait in the queue.
const MAX_CONCURRENT: usize = 2;
const MODEL_ATTEMPTS: u32 = 3;
const CONTEXT_FILE_BYTES: usize = 60_000;
const MAX_AI_COMMANDS: usize = 3;

/// Why a run stopped early.
enum Stop {
    Cancelled,
    Failed(String),
}

impl<E: Into<AppError>> From<E> for Stop {
    fn from(e: E) -> Self {
        Stop::Failed(e.into().to_string())
    }
}

type Flow<T> = std::result::Result<T, Stop>;

pub(crate) fn notify<R: Runtime>(app: &AppHandle<R>, run_id: &str) {
    let _ = app.emit(EVENT_CHANGED, run_id);
}

/// Store the final patch and delete the run's workspace.
pub(crate) fn discard_workspace(state: &AppState, run_id: &str) {
    let found: Option<(String, Option<String>)> = state
        .db
        .conn()
        .query_row("SELECT p.path, r.worktree FROM agent_runs r JOIN projects p ON p.id = r.project_id WHERE r.id = ?1", [run_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok();
    let Some((project, Some(worktree))) = found else { return };
    let dir = PathBuf::from(worktree);
    if dir.is_dir() {
        if let Ok(d) = workspace::diff(&dir) {
            let _ = state.db.conn().execute("UPDATE agent_runs SET patch = ?1 WHERE id = ?2", params![d.patch, run_id]);
        }
        workspace::remove(Path::new(&project), &dir);
    }
    let _ = state.db.conn().execute("UPDATE agent_runs SET worktree = NULL WHERE id = ?1", [run_id]);
}

/// Start queued runs while there is capacity.
pub fn pump<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    loop {
        let running: Vec<String> = state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
        if running.len() >= MAX_CONCURRENT {
            return;
        }
        let queued: Vec<String> = {
            let conn = state.db.conn();
            let Ok(mut stmt) = conn.prepare("SELECT id FROM agent_runs WHERE status = 'queued' ORDER BY created_at, id") else { return };
            let rows = stmt.query_map([], |r| r.get::<_, String>(0));
            match rows {
                Ok(rows) => rows.flatten().collect(),
                Err(_) => return,
            }
        };
        let Some(next) = queued.into_iter().find(|id| !running.contains(id)) else { return };
        start(app.clone(), next);
    }
}

fn start<R: Runtime>(app: AppHandle<R>, run_id: String) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (pause_tx, pause_rx) = watch::channel(false);
    let (decide_tx, decide_rx) = mpsc::unbounded_channel();
    {
        let state = app.state::<AppState>();
        let mut active = state.agents.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.contains_key(&run_id) {
            return;
        }
        active.insert(run_id.clone(), Handle { cancel: cancel_tx, pause: pause_tx, decisions: decide_tx });
        let _ = set_run_status(&state.db.conn(), &run_id, "running", None);
    }
    notify(&app, &run_id);
    tauri::async_runtime::spawn(async move {
        let mut run = Driver { app: app.clone(), run_id: run_id.clone(), cancel: cancel_rx, pause: pause_rx, decisions: decide_rx };
        let outcome = run.drive().await;
        let state = app.state::<AppState>();
        let (status, error) = match outcome {
            Ok(status) => (status, None),
            Err(Stop::Cancelled) => ("cancelled", None),
            Err(Stop::Failed(message)) => ("failed", Some(message)),
        };
        {
            let conn = state.db.conn();
            if status != "failed" {
                // Nothing else will run: close out whatever was still open.
                let leftover = if status == "cancelled" { "cancelled" } else { "skipped" };
                let _ = conn.execute(
                    "UPDATE agent_steps SET status = ?1 WHERE run_id = ?2 AND status IN ('pending', 'running', 'awaiting_approval')",
                    params![leftover, run_id],
                );
            }
            let _ = set_run_status(&conn, &run_id, status, error.as_deref());
            log_event(
                &conn,
                &run_id,
                None,
                "engine",
                if status == "failed" { "error" } else { "info" },
                &match &error {
                    Some(e) => format!("Task failed: {e}"),
                    None => format!("Task {status}"),
                },
            );
            if let Ok(r) = load_run(&conn, &run_id) {
                activity::log(&conn, "agent", &format!("Agent task “{}” {status}", r.title), Some(&r.project_id));
            }
        }
        // A failed run keeps its workspace so it can be resumed; every other ending cleans up.
        if status != "failed" {
            discard_workspace(&state, &run_id);
        }
        state.agents.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&run_id);
        notify(&app, &run_id);
        pump(&app);
    });
}

struct Driver<R: Runtime> {
    app: AppHandle<R>,
    run_id: String,
    cancel: watch::Receiver<bool>,
    pause: watch::Receiver<bool>,
    decisions: mpsc::UnboundedReceiver<(String, bool)>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ResearchReply {
    summary: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct CommandSpec {
    command: String,
    /// "discovered" (from the project's manifests) or "ai" (suggested by the Architect).
    source: String,
    risk: String,
}

#[derive(Serialize, Clone)]
struct OutputEvent<'a> {
    run_id: &'a str,
    step_id: &'a str,
    chunk: &'a str,
}

/// Top-level documentation (README, CONTRIBUTING) as (project-relative path, text), capped in number and size.
/// Symlinks are skipped so nothing outside the project is read.
pub(super) fn project_docs(root: &Path) -> Vec<(String, String)> {
    const MAX_FILES: usize = 2;
    const MAX_CHARS: usize = 6_000;
    let Ok(entries) = std::fs::read_dir(root) else { return vec![] };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_lowercase();
            let stem = lower.split('.').next().unwrap_or("");
            let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            matches!(stem, "readme" | "contributing") && matches!(ext, "" | "md" | "markdown" | "rst" | "txt")
        })
        .collect();
    // README before CONTRIBUTING, then by name, so the choice is stable.
    names.sort_by_key(|n| (!n.to_lowercase().starts_with("readme"), n.clone()));
    names
        .into_iter()
        .filter_map(|name| crate::projects::read_text(&root.join(&name)).map(|text| (name, truncate(&text, MAX_CHARS))))
        .filter(|(_, text)| !text.trim().is_empty())
        .take(MAX_FILES)
        .collect()
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… truncated …", &text[..cut])
}

impl<R: Runtime> Driver<R> {
    fn log(&self, step: Option<&Step>, role: &str, level: &str, message: &str) {
        let state = self.app.state::<AppState>();
        log_event(&state.db.conn(), &self.run_id, step.map(|s| s.id.as_str()), role, level, message);
    }

    fn update(&self, step: &Step, status: &str, output: Option<&str>, error: Option<&str>) -> Flow<()> {
        let state = self.app.state::<AppState>();
        set_step(&state.db.conn(), &step.id, status, output, error)?;
        notify(&self.app, &self.run_id);
        Ok(())
    }

    /// The next step whose dependencies are satisfied. Steps blocked by a failed dependency are skipped.
    fn next_step(&self) -> Flow<Option<Step>> {
        let state = self.app.state::<AppState>();
        let conn = state.db.conn();
        let steps = load_steps(&conn, &self.run_id)?;
        for step in steps.iter().filter(|s| s.status == "pending") {
            let deps: Vec<&Step> = steps.iter().filter(|s| step.depends_on.contains(&s.id)).collect();
            // A failed test command is a result to report, not a reason to stop.
            let done = |d: &Step| matches!(d.status.as_str(), "completed" | "skipped") || (d.kind == "command" && d.status == "failed");
            if deps.iter().all(|d| done(d)) {
                return Ok(Some(step.clone()));
            }
            if deps.iter().any(|d| !done(d) && matches!(d.status.as_str(), "failed" | "rejected" | "cancelled")) {
                set_step(&conn, &step.id, "skipped", None, Some("A step this depends on did not complete"))?;
            }
        }
        Ok(None)
    }

    fn output_of(&self, kind: &str) -> Flow<String> {
        let state = self.app.state::<AppState>();
        let steps = load_steps(&state.db.conn(), &self.run_id)?;
        Ok(steps.into_iter().find(|s| s.kind == kind).map(|s| s.output).unwrap_or_default())
    }

    /// Wait for the user's decision on a step. Nothing proceeds without one.
    async fn decision(&mut self, step: &Step, role: &str, what: &str) -> Flow<bool> {
        self.update(step, "awaiting_approval", None, None)?;
        self.log(Some(step), role, "approval", &format!("Waiting for approval: {what}"));
        crate::platform::notify::attention(&self.app, "NEXUS: an agent task needs your approval", what);
        loop {
            tokio::select! {
                _ = self.cancel.changed() => return Err(Stop::Cancelled),
                msg = self.decisions.recv() => match msg {
                    Some((id, approved)) if id == step.id => {
                        self.log(Some(step), "user", "approval", &format!("{}: {what}", if approved { "Approved" } else { "Rejected" }));
                        return Ok(approved);
                    }
                    Some(_) => continue,
                    None => return Err(Stop::Cancelled),
                },
            }
        }
    }

    /// One model call with retries. Provider errors and unusable replies are retried with backoff.
    async fn ask<T: DeserializeOwned>(
        &mut self,
        client: &Arc<dyn ModelClient>,
        step: &Step,
        role: Role,
        system: String,
        user: String,
    ) -> Flow<T> {
        let app = self.app.clone();
        let state = app.state::<AppState>();
        let mut last = String::new();
        for attempt in 1..=MODEL_ATTEMPTS {
            state.db.conn().execute("UPDATE agent_steps SET attempts = ?1 WHERE id = ?2", params![attempt, step.id])?;
            self.log(Some(step), role.id(), "model", &format!("Asking the model (attempt {attempt} of {MODEL_ATTEMPTS})"));
            let reply = tokio::select! {
                _ = self.cancel.changed() => return Err(Stop::Cancelled),
                r = client.complete(&state, system.clone(), user.clone()) => r,
            };
            match reply.and_then(|r| roles::parse_reply::<T>(&r.content).map(|parsed| (parsed, r))) {
                Ok((parsed, r)) => {
                    self.log(
                        Some(step),
                        role.id(),
                        "model",
                        &format!(
                            "{} replied in {:.1}s ({} in / {} out tokens)",
                            r.model,
                            r.duration_ms as f64 / 1000.0,
                            r.input_tokens.unwrap_or(0),
                            r.output_tokens.unwrap_or(0)
                        ),
                    );
                    return Ok(parsed);
                }
                Err(e) => {
                    last = e.to_string();
                    self.log(Some(step), role.id(), "error", &format!("Attempt {attempt} failed: {last}"));
                    if attempt < MODEL_ATTEMPTS {
                        let wait = Duration::from_millis(state.agents.retry_delay() * u64::from(attempt));
                        tokio::select! {
                            _ = self.cancel.changed() => return Err(Stop::Cancelled),
                            _ = tokio::time::sleep(wait) => {}
                        }
                    }
                }
            }
        }
        Err(Stop::Failed(format!(
            "The {} agent could not get a usable answer after {MODEL_ATTEMPTS} attempts. Last error: {last}",
            role.name()
        )))
    }

    async fn drive(&mut self) -> Flow<&'static str> {
        let app = self.app.clone();
        let state = app.state::<AppState>();
        let run = load_run(&state.db.conn(), &self.run_id)?;
        let project = project_root(&state, &run.project_id)?;
        let client: Arc<dyn ModelClient> = match state.agents.client.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(scripted) => scripted,
            None => Arc::new(ProviderClient { provider_id: run.provider_id.clone(), model: run.model.clone() }),
        };

        // Isolated workspace: created once, reused when a run is resumed.
        let worktree = match run.worktree.as_deref().map(PathBuf::from).filter(|d| d.is_dir()) {
            Some(dir) => dir,
            None => {
                let dir = workspace_dir(&state, &self.run_id);
                let base = workspace::create(&project, &dir)?;
                state.db.conn().execute(
                    "UPDATE agent_runs SET worktree = ?1, base_commit = ?2 WHERE id = ?3",
                    params![dir.to_string_lossy(), base, self.run_id],
                )?;
                self.log(None, "engine", "tool", &format!("Created isolated workspace at commit {}", &base[..base.len().min(10)]));
                dir
            }
        };

        loop {
            if *self.cancel.borrow() {
                return Err(Stop::Cancelled);
            }
            if *self.pause.borrow() {
                set_run_status(&state.db.conn(), &self.run_id, "paused", None)?;
                self.log(None, "engine", "info", "Paused before the next step");
                notify(&app, &self.run_id);
                loop {
                    tokio::select! {
                        _ = self.cancel.changed() => return Err(Stop::Cancelled),
                        changed = self.pause.changed() => if changed.is_err() || !*self.pause.borrow() { break },
                    }
                }
                set_run_status(&state.db.conn(), &self.run_id, "running", None)?;
                self.log(None, "engine", "info", "Resumed");
                notify(&app, &self.run_id);
            }
            let Some(step) = self.next_step()? else { return Ok("completed") };

            let result = match step.kind.as_str() {
                "research" => self.research(&client, &step, &run.project_id, &run.outcome).await,
                "plan" => self.plan(&client, &step, &run.project_id, &run.outcome).await,
                "plan_approval" => match self.decision(&step, "user", "the Architect's plan").await {
                    Ok(true) => self.update(&step, "completed", None, None),
                    Ok(false) => {
                        self.update(&step, "rejected", None, None)?;
                        return Ok("rejected");
                    }
                    Err(e) => Err(e),
                },
                "develop" => self.develop(&client, &step, &worktree, &run.outcome).await,
                "discover" => self.discover(&step, &worktree),
                "command" => self.command(&step, &worktree).await,
                "review" => self.review(&client, &step, &worktree, &run.outcome).await,
                "apply" => match self.apply(&step, &project, &worktree).await {
                    Ok(true) => Ok(()),
                    Ok(false) => return Ok("discarded"),
                    Err(e) => Err(e),
                },
                other => Err(Stop::Failed(format!("Unknown step kind “{other}”"))),
            };
            match result {
                Ok(()) => {}
                Err(Stop::Cancelled) => {
                    let _ = set_step(&state.db.conn(), &step.id, "cancelled", None, None);
                    return Err(Stop::Cancelled);
                }
                Err(Stop::Failed(message)) => {
                    let _ = set_step(&state.db.conn(), &step.id, "failed", None, Some(&message));
                    self.log(Some(&step), &step.role, "error", &message);
                    return Err(Stop::Failed(message));
                }
            }
        }
    }

    // ------------------------------------------------------------ steps

    async fn research(&mut self, client: &Arc<dyn ModelClient>, step: &Step, project_id: &str, outcome: &str) -> Flow<()> {
        roles::require(Role::Research, Capability::ReadProject)?;
        self.update(step, "running", None, None)?;
        let app = self.app.clone();
        let state = app.state::<AppState>();
        intel::ensure_index(&state, project_id).await?;
        let sources = intel::retrieve_sources(&state, project_id, outcome.to_string(), None).await?;
        self.log(Some(step), Role::Research.id(), "tool", &format!("Searched the code index: {} relevant passages", sources.len()));
        let mut listed: Vec<_> = sources
            .iter()
            .map(|s| json!({"n": s.n, "path": s.path, "start_line": s.start_line, "end_line": s.end_line, "reason": s.reason}))
            .collect();
        let mut excerpts: String =
            sources.iter().map(|s| format!("[{}] {} (lines {}-{})\n{}\n\n", s.n, s.path, s.start_line, s.end_line, s.text)).collect();
        // The code index holds source files only, so the project's own documentation is added here.
        let docs = project_docs(&project_root(&state, project_id)?);
        if !docs.is_empty() {
            self.log(
                Some(step),
                Role::Research.id(),
                "tool",
                &format!("Read project documentation: {}", docs.iter().map(|d| d.0.as_str()).collect::<Vec<_>>().join(", ")),
            );
        }
        for (path, text) in docs {
            let n = listed.len() + 1;
            let lines = text.lines().count().max(1);
            excerpts.push_str(&format!("[{n}] {path} (lines 1-{lines})\n{text}\n\n"));
            listed.push(json!({"n": n, "path": path, "start_line": 1, "end_line": lines, "reason": "project documentation"}));
        }
        let summary = if listed.is_empty() {
            "No source passages matched the task description, so there is no evidence from the existing code.".to_string()
        } else {
            let (system, user) = roles::research_prompt(outcome, &truncate(&excerpts, 24_000));
            self.ask::<ResearchReply>(client, step, Role::Research, system, user).await?.summary
        };
        self.update(step, "completed", Some(&json!({"summary": summary, "sources": listed}).to_string()), None)
    }

    async fn plan(&mut self, client: &Arc<dyn ModelClient>, step: &Step, project_id: &str, outcome: &str) -> Flow<()> {
        roles::require(Role::Architect, Capability::ReadProject)?;
        self.update(step, "running", None, None)?;
        let app = self.app.clone();
        let state = app.state::<AppState>();
        let (project, structure) = {
            let conn = state.db.conn();
            let analysis: Option<String> = conn.query_row("SELECT analysis FROM projects WHERE id = ?1", [project_id], |r| r.get(0))?;
            let summary = analysis
                .and_then(|a| serde_json::from_str::<serde_json::Value>(&a).ok())
                .and_then(|v| v.get("summary").and_then(|s| s.as_str()).map(String::from))
                .unwrap_or_else(|| "No analysis available.".into());
            let graph = intel::graph_for(&conn, project_id, "module", 2, false)?;
            let mut text = String::new();
            for n in &graph.nodes {
                let deps: Vec<&str> = graph.edges.iter().filter(|e| e.from == n.id).map(|e| e.to.as_str()).collect();
                text.push_str(&format!(
                    "- {} ({} files, {} lines) depends on: {}\n",
                    n.id,
                    n.files,
                    n.lines,
                    if deps.is_empty() { "nothing".into() } else { deps.join(", ") }
                ));
            }
            for c in &graph.cycles {
                text.push_str(&format!("- dependency cycle: {}\n", c.join(" <-> ")));
            }
            (summary, text)
        };
        self.log(Some(step), Role::Architect.id(), "tool", "Read the project analysis and module map");
        let research: serde_json::Value = serde_json::from_str(&self.output_of("research")?).unwrap_or_default();
        let sources = research.get("sources").map(|s| s.to_string()).unwrap_or_default();
        let (system, user) = roles::architect_prompt(
            outcome,
            &project,
            &structure,
            research.get("summary").and_then(|s| s.as_str()).unwrap_or(""),
            &sources,
        );
        let mut plan: Plan = self.ask(client, step, Role::Architect, system, user).await?;
        if plan.steps.is_empty() {
            return Err(Stop::Failed("The Architect returned a plan with no steps.".into()));
        }
        plan.files.truncate(12);
        plan.test_commands.retain(|c| !c.trim().is_empty());
        plan.test_commands.truncate(MAX_AI_COMMANDS);
        self.update(step, "completed", Some(&serde_json::to_string(&plan)?), None)
    }

    async fn develop(&mut self, client: &Arc<dyn ModelClient>, step: &Step, worktree: &Path, outcome: &str) -> Flow<()> {
        roles::require(Role::Developer, Capability::ReadProject)?;
        self.update(step, "running", None, None)?;
        let plan: Plan = serde_json::from_str(&self.output_of("plan")?).unwrap_or_default();
        let mut files = String::new();
        for path in &plan.files {
            match workspace::read_file(worktree, path, CONTEXT_FILE_BYTES) {
                Some(text) => files.push_str(&format!("### {path}\n```\n{text}\n```\n\n")),
                None => files.push_str(&format!("### {path}\n(does not exist yet, or cannot be read as text)\n\n")),
            }
        }
        self.log(Some(step), Role::Developer.id(), "tool", &format!("Read {} files from the workspace", plan.files.len()));
        let (system, user) = roles::developer_prompt(outcome, &plan, &files);
        let changes: Changes = self.ask(client, step, Role::Developer, system, user).await?;

        roles::require(Role::Developer, Capability::WriteWorkspace)?;
        if changes.changes.len() > workspace::MAX_CHANGED_FILES {
            return Err(Stop::Failed(format!(
                "The Developer tried to change {} files; the limit is {}.",
                changes.changes.len(),
                workspace::MAX_CHANGED_FILES
            )));
        }
        for change in &changes.changes {
            workspace::write_file(worktree, &change.path, &change.content)?;
            self.log(Some(step), Role::Developer.id(), "tool", &format!("Wrote {} in the workspace", change.path));
        }
        let diff = workspace::diff(worktree)?;
        self.update(step, "completed", Some(&json!({"summary": changes.summary, "files": diff.files}).to_string()), None)
    }

    fn discover(&mut self, step: &Step, worktree: &Path) -> Flow<()> {
        roles::require(Role::Testing, Capability::ReadProject)?;
        self.update(step, "running", None, None)?;
        let plan: Plan = serde_json::from_str(&self.output_of("plan")?).unwrap_or_default();
        let mut commands: Vec<CommandSpec> = workspace::discover_tests(worktree)
            .into_iter()
            .map(|command| CommandSpec { risk: tasks::risk_of(&command).into(), command, source: "discovered".into() })
            .collect();
        for command in plan.test_commands {
            let command = command.trim().to_string();
            if !commands.iter().any(|c| c.command == command) {
                commands.push(CommandSpec { risk: tasks::risk_of(&command).into(), command, source: "ai".into() });
            }
        }
        let state = self.app.state::<AppState>();
        {
            // Each command becomes its own step, so each gets its own approval and its own record.
            let conn = state.db.conn();
            let n = commands.len() as i64;
            conn.execute(
                "UPDATE agent_steps SET position = position + ?1 WHERE run_id = ?2 AND position > ?3",
                params![n, self.run_id, step.position],
            )?;
            let mut ids = vec![];
            for (i, c) in commands.iter().enumerate() {
                ids.push(insert_step(
                    &conn,
                    &self.run_id,
                    step.position + 1 + i as i64,
                    Role::Testing.id(),
                    "command",
                    &format!("Run: {}", c.command),
                    &serde_json::to_string(c)?,
                    std::slice::from_ref(&step.id),
                )?);
            }
            // The review waits for every command.
            for review in load_steps(&conn, &self.run_id)?.into_iter().filter(|s| s.kind == "review") {
                let mut deps = review.depends_on;
                deps.extend(ids.iter().cloned());
                conn.execute("UPDATE agent_steps SET depends_on = ?1 WHERE id = ?2", params![serde_json::to_string(&deps)?, review.id])?;
            }
        }
        self.log(Some(step), Role::Testing.id(), "tool", &format!("Found {} test command(s); each needs your approval", commands.len()));
        self.update(step, "completed", Some(&serde_json::to_string(&commands)?), None)
    }

    async fn command(&mut self, step: &Step, worktree: &Path) -> Flow<()> {
        roles::require(Role::Testing, Capability::RunCommand)?;
        let spec: CommandSpec = serde_json::from_str(&step.detail).unwrap_or_default();
        if spec.command.trim().is_empty() {
            return self.update(step, "skipped", None, Some("Empty command"));
        }
        if !self.decision(step, Role::Testing.id(), &format!("run `{}`", spec.command)).await? {
            return self.update(step, "skipped", None, None);
        }
        self.update(step, "running", Some(""), None)?;
        self.log(Some(step), Role::Testing.id(), "tool", &format!("Running `{}` in the workspace", spec.command));
        let (app, run_id, step_id) = (self.app.clone(), self.run_id.clone(), step.id.clone());
        let exec = jobs::run_command(&spec.command, Some(worktree), true, &mut self.cancel, |chunk| {
            let _ = app.emit(EVENT_OUTPUT, OutputEvent { run_id: &run_id, step_id: &step_id, chunk });
        })
        .await;
        match exec {
            Err(e) => self.update(step, "failed", Some(&format!("Could not start the command: {e}")), Some("could not start")),
            Ok(e) if e.cancelled => {
                let state = self.app.state::<AppState>();
                set_step(&state.db.conn(), &step.id, "cancelled", Some(&e.output), None)?;
                Err(Stop::Cancelled)
            }
            Ok(e) if e.code == Some(0) => {
                self.log(Some(step), Role::Testing.id(), "info", "Command passed (exit code 0)");
                self.update(step, "completed", Some(&e.output), None)
            }
            Ok(e) => {
                let code = e.code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into());
                self.log(Some(step), Role::Testing.id(), "error", &format!("Command failed with exit code {code}"));
                self.update(step, "failed", Some(&e.output), Some(&format!("exit code {code}")))
            }
        }
    }

    async fn review(&mut self, client: &Arc<dyn ModelClient>, step: &Step, worktree: &Path, outcome: &str) -> Flow<()> {
        roles::require(Role::Review, Capability::ReadProject)?;
        self.update(step, "running", None, None)?;
        let diff = workspace::diff(worktree)?;
        if diff.files.is_empty() {
            let report = ReviewReport {
                verdict: "no_changes".into(),
                summary: "The Developer produced no file changes, so there is nothing to review.".into(),
                issues: vec![],
            };
            return self.update(step, "completed", Some(&serde_json::to_string(&report)?), None);
        }
        let plan: Plan = serde_json::from_str(&self.output_of("plan")?).unwrap_or_default();
        let tests: String = {
            let state = self.app.state::<AppState>();
            let steps = load_steps(&state.db.conn(), &self.run_id)?;
            let lines: Vec<String> = steps
                .iter()
                .filter(|s| s.kind == "command")
                .map(|s| {
                    let tail: Vec<&str> = s.output.lines().rev().take(30).collect();
                    format!(
                        "{} -> {}{}\n{}",
                        s.title,
                        s.status,
                        s.error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default(),
                        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                    )
                })
                .collect();
            if lines.is_empty() {
                "No test commands were run.".into()
            } else {
                lines.join("\n\n")
            }
        };
        self.log(Some(step), Role::Review.id(), "tool", &format!("Read the diff ({} files) and the test results", diff.files.len()));
        let (system, user) = roles::review_prompt(outcome, &plan, &truncate(&diff.patch, 60_000), &truncate(&tests, 12_000));
        let report: ReviewReport = self.ask(client, step, Role::Review, system, user).await?;
        self.update(step, "completed", Some(&serde_json::to_string(&report)?), None)
    }

    /// The only place a run touches the project. Returns false when the user discards the changes.
    async fn apply(&mut self, step: &Step, project: &Path, worktree: &Path) -> Flow<bool> {
        let diff = workspace::diff(worktree)?;
        if diff.files.is_empty() {
            self.update(step, "completed", Some("There were no changes to apply."), None)?;
            return Ok(true);
        }
        if !self.decision(step, "user", &format!("apply {} changed file(s) to the project", diff.files.len())).await? {
            self.update(step, "rejected", Some("Changes discarded. Your project was not modified."), None)?;
            return Ok(false);
        }
        self.update(step, "running", None, None)?;
        let scratch = self.app.state::<AppState>().data_dir.join("scratch");
        workspace::apply(project, &diff.patch, &scratch)?;
        self.log(
            Some(step),
            "engine",
            "tool",
            &format!("Applied {} file(s) to the project working tree (not staged, not committed)", diff.files.len()),
        );
        self.update(
            step,
            "completed",
            Some(&format!("Applied {} file(s) to your project. Nothing was staged or committed.", diff.files.len())),
            None,
        )?;
        Ok(true)
    }
}
