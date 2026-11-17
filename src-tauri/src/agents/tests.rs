//! End-to-end tests of the agent engine on a mock Tauri app, a real Git repository and a scripted
//! model. Only the model is scripted: workspaces, commands, diffs and applies are all real.
#![cfg(unix)]

use super::*;
use crate::ai::{ChatResponse, ModelFuture};
use crate::git::git;
use std::fs;
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;

#[derive(Default)]
struct Scripted {
    replies: Mutex<HashMap<&'static str, String>>,
    /// Remaining failures to inject per role.
    failures: Mutex<HashMap<&'static str, u32>>,
    calls: Mutex<Vec<&'static str>>,
}

impl Scripted {
    fn set(&self, role: &'static str, reply: &str) {
        self.replies.lock().unwrap().insert(role, reply.to_string());
    }
    fn fail(&self, role: &'static str, times: u32) {
        self.failures.lock().unwrap().insert(role, times);
    }
    fn calls(&self, role: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|r| **r == role).count()
    }
}

impl ModelClient for Scripted {
    fn complete<'a>(&'a self, _state: &'a AppState, system: String, _user: String) -> ModelFuture<'a> {
        Box::pin(async move {
            let role = ["research", "architect", "developer", "review"]
                .into_iter()
                .find(|r| system.to_lowercase().contains(&format!("{r} agent")))
                .expect("prompt names its role");
            self.calls.lock().unwrap().push(role);
            if let Some(left) = self.failures.lock().unwrap().get_mut(role).filter(|n| **n > 0) {
                *left -= 1;
                return Err(AppError::Msg("Provider returned 503 Service Unavailable".into()));
            }
            let content = self.replies.lock().unwrap().get(role).cloned().unwrap_or_default();
            Ok(ChatResponse { content, model: "scripted".into(), input_tokens: Some(10), output_tokens: Some(5), duration_ms: 1 })
        })
    }
}

struct Harness {
    app: tauri::App<MockRuntime>,
    project: PathBuf,
    client: Arc<Scripted>,
}

const ORIGINAL: &str = "pub fn greet() -> &'static str {\n    \"Hello\"\n}\n";
const CHANGED: &str = "pub fn greet() -> &'static str {\n    \"Hello, NEXUS\"\n}\n";

impl Harness {
    fn new() -> Self {
        let project = workspace::tests::repo(&[("src/greet.rs", ORIGINAL), ("user.txt", "mine\n")]);
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let state = AppState::for_tests();
        state
            .db
            .conn()
            .execute("INSERT INTO projects (id, name, path, added_at) VALUES ('p', 'demo', ?1, '')", [project.to_string_lossy()])
            .unwrap();
        let client = Arc::new(Scripted::default());
        client.set("research", r#"{"summary": "greet is defined in src/greet.rs [1]"}"#);
        client.set(
            "architect",
            r#"Plan follows. {"analysis": "One small module.", "issues": ["No tests exist"],
            "steps": [{"title": "Change the greeting", "detail": "Return the new text"}],
            "files": ["src/greet.rs"], "test_commands": ["grep -q NEXUS src/greet.rs"]}"#,
        );
        client.set(
            "developer",
            &serde_json::json!({"summary": "Updated the greeting", "changes": [
            {"path": "src/greet.rs", "content": CHANGED}, {"path": "docs/NOTES.md", "content": "greeting changed\n"}]})
            .to_string(),
        );
        client.set("review", r#"{"verdict": "approve", "summary": "Matches the task.", "issues": []}"#);
        *state.agents.client.lock().unwrap() = Some(client.clone());
        state.agents.retry_delay_ms.store(5, Ordering::Relaxed);
        app.manage(state);
        Harness { app, project, client }
    }

    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn create(&self) -> String {
        create_agent_run(
            self.app.handle().clone(),
            self.state(),
            "p".into(),
            "Change the greet function to say Hello, NEXUS".into(),
            "scripted".into(),
            None,
            None,
        )
        .unwrap()
        .id
    }

    fn detail(&self, run: &str) -> RunDetail {
        get_agent_run(self.state(), run.to_string()).unwrap()
    }

    fn wait(&self, run: &str, what: &str, pred: impl Fn(&RunDetail) -> bool) -> RunDetail {
        let started = Instant::now();
        loop {
            let d = self.detail(run);
            if pred(&d) {
                return d;
            }
            if started.elapsed() > Duration::from_secs(20) {
                let steps: Vec<String> = d.steps.iter().map(|s| format!("{}={}", s.kind, s.status)).collect();
                panic!("timed out waiting for {what}: run={} error={:?} steps={steps:?}", d.run.status, d.run.error);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Wait until a step of this kind is waiting for approval, and return it.
    fn gate(&self, run: &str, kind: &str) -> Step {
        let d = self.wait(run, &format!("{kind} approval"), |d| d.steps.iter().any(|s| s.kind == kind && s.status == "awaiting_approval"));
        d.steps.into_iter().find(|s| s.kind == kind && s.status == "awaiting_approval").unwrap()
    }

    fn decide(&self, run: &str, step: &Step, approved: bool) {
        agent_decide(self.state(), run.to_string(), step.id.clone(), approved).unwrap();
    }

    fn finished(&self, run: &str, status: &str) -> RunDetail {
        self.wait(run, &format!("run {status}"), |d| d.run.status == status && !self.state().agents.is_active(run))
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.project.join(path)).unwrap_or_else(|_| "<missing>".into())
    }

    fn workspace(&self, run: &str) -> PathBuf {
        workspace_dir(&self.state(), run)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.state().data_dir);
        let _ = fs::remove_dir_all(&self.project);
    }
}

fn step<'a>(d: &'a RunDetail, kind: &str) -> &'a Step {
    d.steps.iter().find(|s| s.kind == kind).unwrap_or_else(|| panic!("no {kind} step"))
}

#[test]
fn full_workflow_needs_every_approval_and_never_touches_the_project_early() {
    let h = Harness::new();
    let run = h.create();

    // 1. Plan gate: only read-only agents have run.
    let plan_gate = h.gate(&run, "plan_approval");
    let d = h.detail(&run);
    assert_eq!(d.run.pending_approvals, 1);
    assert_eq!((h.client.calls("research"), h.client.calls("architect"), h.client.calls("developer")), (1, 1, 0));
    assert!(step(&d, "research").output.contains("src/greet.rs"));
    assert!(step(&d, "plan").output.contains("Change the greeting"));
    assert_eq!(h.read("src/greet.rs"), ORIGINAL);
    // A decision for a step that is not waiting is refused.
    let develop_id = step(&d, "develop").id.clone();
    assert!(agent_decide(h.state(), run.clone(), develop_id, true).is_err());
    h.decide(&run, &plan_gate, true);

    // 2. Command gate: the Developer wrote to the workspace only, and the command has not run.
    let command = h.gate(&run, "command");
    assert!(command.detail.contains("grep -q NEXUS") && command.detail.contains("\"source\":\"ai\""));
    assert_eq!(command.output, "");
    assert_eq!(fs::read_to_string(h.workspace(&run).join("src/greet.rs")).unwrap(), CHANGED);
    assert_eq!(h.read("src/greet.rs"), ORIGINAL, "project untouched while the workspace changes");
    assert_eq!(h.read("docs/NOTES.md"), "<missing>");
    h.decide(&run, &command, true);

    // 3. Apply gate: tests ran, review done, exact diff available, project still untouched.
    let apply = h.gate(&run, "apply");
    let d = h.detail(&run);
    assert_eq!(step(&d, "command").status, "completed");
    assert!(step(&d, "review").output.contains("approve"));
    let diff = tauri::async_runtime::block_on(agent_diff(h.state(), run.clone())).unwrap();
    assert_eq!(
        diff.files.iter().map(|f| (f.status.as_str(), f.path.as_str())).collect::<Vec<_>>(),
        [("added", "docs/NOTES.md"), ("modified", "src/greet.rs")]
    );
    assert!(diff.patch.contains("+    \"Hello, NEXUS\""));
    assert_eq!(h.read("src/greet.rs"), ORIGINAL);
    // The user keeps working in their project while the agent waits.
    fs::write(h.project.join("user.txt"), "mine, still editing\n").unwrap();
    h.decide(&run, &apply, true);

    // 4. Applied: project updated, user's work intact, nothing staged, workspace gone, trace kept.
    let d = h.finished(&run, "completed");
    assert_eq!(h.read("src/greet.rs"), CHANGED);
    assert_eq!(h.read("docs/NOTES.md"), "greeting changed\n");
    assert_eq!(h.read("user.txt"), "mine, still editing\n");
    assert!(git(&h.project, &["diff", "--cached", "--name-only"]).unwrap().trim().is_empty());
    assert_eq!(git(&h.project, &["rev-list", "--count", "HEAD"]).unwrap().trim(), "1", "no commit was made");
    assert!(!h.workspace(&run).exists());
    assert!(d.steps.iter().all(|s| s.status == "completed"), "{:?}", d.steps.iter().map(|s| (&s.kind, &s.status)).collect::<Vec<_>>());
    for role in ["research", "architect", "developer", "testing", "review", "user", "engine"] {
        assert!(d.events.iter().any(|e| e.role == role), "trace has no {role} event");
    }
    assert_eq!(d.events.iter().filter(|e| e.level == "approval" && e.message.starts_with("Approved")).count(), 3);
    // The diff is still inspectable from history.
    let stored = tauri::async_runtime::block_on(agent_diff(h.state(), run.clone())).unwrap();
    assert_eq!(stored.files.len(), 2);
}

#[test]
fn rejecting_the_plan_or_the_changes_leaves_the_project_untouched() {
    let h = Harness::new();
    let run = h.create();
    h.decide(&run, &h.gate(&run, "plan_approval"), false);
    let d = h.finished(&run, "rejected");
    assert_eq!(h.client.calls("developer"), 0, "nothing is implemented without an approved plan");
    assert_eq!(step(&d, "plan_approval").status, "rejected");
    assert!(d.steps.iter().filter(|s| s.position > 3).all(|s| s.status == "skipped"));
    assert!(agent_resume(h.app.handle().clone(), h.state(), run.clone()).is_err(), "a rejected task stays rejected");

    let run = h.create();
    h.decide(&run, &h.gate(&run, "plan_approval"), true);
    h.decide(&run, &h.gate(&run, "command"), false); // skip the command
    h.decide(&run, &h.gate(&run, "apply"), false); // discard the changes
    let d = h.finished(&run, "discarded");
    assert_eq!((step(&d, "command").status.as_str(), step(&d, "command").output.as_str()), ("skipped", ""));
    assert_eq!(h.read("src/greet.rs"), ORIGINAL);
    assert_eq!(h.read("docs/NOTES.md"), "<missing>");
    assert!(!h.workspace(&run).exists());
    assert!(git(&h.project, &["worktree", "list"]).unwrap().lines().count() == 1, "no stray worktrees");
}

#[test]
fn provider_failures_are_retried_then_fail_cleanly_and_can_be_resumed() {
    let h = Harness::new();
    h.client.fail("architect", 2);
    let run = h.create();
    h.gate(&run, "plan_approval");
    let d = h.detail(&run);
    assert_eq!(step(&d, "plan").attempts, 3, "two failures, then success");
    assert_eq!(d.events.iter().filter(|e| e.level == "error" && e.message.contains("503")).count(), 2);
    agent_cancel(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    h.finished(&run, "cancelled");

    // A provider that stays down fails the run with a clear message.
    h.client.fail("architect", 99);
    let run = h.create();
    let d = h.finished(&run, "failed");
    assert!(d.run.error.as_deref().unwrap().contains("after 3 attempts"), "{:?}", d.run.error);
    assert_eq!(step(&d, "plan").status, "failed");
    assert_eq!(step(&d, "research").status, "completed");
    assert_eq!(step(&d, "develop").status, "pending");
    assert!(h.workspace(&run).exists(), "a failed run keeps its workspace for resuming");

    // An unparseable reply counts as a failure too.
    h.client.fail("architect", 0);
    h.client.set("architect", "I would rather chat than return JSON.");
    agent_resume(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    let d = h.finished(&run, "failed");
    assert!(d.run.error.as_deref().unwrap().contains("JSON"));

    // Fixed provider: resume continues without repeating finished work.
    let research_calls = h.client.calls("research");
    h.client.set("architect", r#"{"steps": [{"title": "Do it", "detail": ""}], "files": ["src/greet.rs"]}"#);
    agent_resume(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    h.gate(&run, "plan_approval");
    assert_eq!(h.client.calls("research"), research_calls, "finished steps are not re-run");
}

#[test]
fn cancelling_kills_a_running_command() {
    let h = Harness::new();
    h.client.set("architect", r#"{"steps": [{"title": "x", "detail": ""}], "files": [], "test_commands": ["sleep 30 & sleep 30"]}"#);
    let run = h.create();
    h.decide(&run, &h.gate(&run, "plan_approval"), true);
    h.decide(&run, &h.gate(&run, "command"), true);
    h.wait(&run, "command running", |d| step(d, "command").status == "running");
    let started = Instant::now();
    agent_cancel(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    let d = h.finished(&run, "cancelled");
    assert!(started.elapsed() < Duration::from_secs(5), "cancel must not wait for the command");
    assert_eq!(step(&d, "command").status, "cancelled");
    assert!(step(&d, "command").output.contains("[cancelled]"));
    assert_eq!(step(&d, "apply").status, "cancelled");
    assert_eq!(h.read("src/greet.rs"), ORIGINAL);
    assert!(!h.workspace(&run).exists());
}

#[test]
fn a_developer_cannot_write_outside_its_workspace() {
    let h = Harness::new();
    let outside = h.project.parent().unwrap().join("nexus-escape-proof.txt");
    let _ = fs::remove_file(&outside);
    h.client.set("developer", r#"{"summary": "sneaky", "changes": [{"path": "../nexus-escape-proof.txt", "content": "escaped"}]}"#);
    let run = h.create();
    h.decide(&run, &h.gate(&run, "plan_approval"), true);
    let d = h.finished(&run, "failed");
    assert!(d.run.error.as_deref().unwrap().contains("Refused unsafe path"), "{:?}", d.run.error);
    assert!(!outside.exists(), "nothing was written outside the workspace");
    assert_eq!(h.read("src/greet.rs"), ORIGINAL);

    // Git metadata is off limits as well.
    h.client.set("developer", r#"{"summary": "hooks", "changes": [{"path": ".git/hooks/pre-commit", "content": "rm -rf /"}]}"#);
    agent_resume(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    h.wait(&run, "second failure", |d| d.run.status == "failed" && d.events.iter().any(|e| e.message.contains("Git metadata")));
    assert!(!h.project.join(".git/hooks/pre-commit").exists());
}

#[test]
fn applying_never_overwrites_the_users_own_edits() {
    let h = Harness::new();
    h.client.set("architect", r#"{"steps": [{"title": "x", "detail": ""}], "files": ["src/greet.rs"]}"#);
    let run = h.create();
    h.decide(&run, &h.gate(&run, "plan_approval"), true);
    let apply = h.gate(&run, "apply");
    // Meanwhile the user edits the same file and does not commit it.
    let mine = "pub fn greet() -> &'static str {\n    \"my own unsaved rewrite\"\n}\n";
    fs::write(h.project.join("src/greet.rs"), mine).unwrap();
    h.decide(&run, &apply, true);

    let d = h.finished(&run, "failed");
    assert!(d.run.error.as_deref().unwrap().contains("nothing was changed"), "{:?}", d.run.error);
    assert_eq!(h.read("src/greet.rs"), mine, "the user's edit must survive");
    assert_eq!(h.read("docs/NOTES.md"), "<missing>", "a failed apply applies nothing");

    // Once the conflict is gone, the same run can be resumed and applied.
    git(&h.project, &["checkout", "--", "src/greet.rs"]).unwrap();
    agent_resume(h.app.handle().clone(), h.state(), run.clone()).unwrap();
    h.decide(&run, &h.gate(&run, "apply"), true);
    h.finished(&run, "completed");
    assert_eq!(h.read("src/greet.rs"), CHANGED);
}

#[test]
fn interrupted_runs_are_marked_and_resume_from_the_last_finished_step() {
    let h = Harness::new();
    // State as it would be on disk if the app died while the Architect was working.
    {
        let state = h.state();
        let conn = state.db.conn();
        conn.execute(
            "INSERT INTO agent_runs (id, project_id, title, outcome, provider_id, status, created_at, updated_at)
             VALUES ('r', 'p', 't', 'Change the greet function', 'scripted', 'running', '', '')",
            [],
        )
        .unwrap();
        insert_pipeline(&conn, "r").unwrap();
        conn.execute("UPDATE agent_steps SET status = 'completed', output = '{\"summary\":\"from before the crash\",\"sources\":[]}' WHERE run_id = 'r' AND kind = 'research'", []).unwrap();
        conn.execute("UPDATE agent_steps SET status = 'running' WHERE run_id = 'r' AND kind = 'plan'", []).unwrap();
    }
    recover_interrupted(&h.state().db).unwrap();
    let d = h.detail("r");
    assert_eq!(d.run.status, "interrupted");
    assert!(d.run.error.as_deref().unwrap().contains("Resume"));
    assert_eq!(step(&d, "plan").status, "pending");
    assert!(!h.state().agents.is_active("r"), "nothing restarts by itself");
    assert_eq!(h.client.calls("architect"), 0);

    agent_resume(h.app.handle().clone(), h.state(), "r".into()).unwrap();
    h.gate("r", "plan_approval");
    assert_eq!(h.client.calls("research"), 0, "research finished before the crash and is not repeated");
    assert_eq!(h.client.calls("architect"), 1);
    assert!(step(&h.detail("r"), "research").output.contains("from before the crash"));
}

#[test]
fn queue_limits_concurrency_and_pause_holds_before_the_next_step() {
    let h = Harness::new();
    let (a, b, c) = (h.create(), h.create(), h.create());
    h.gate(&a, "plan_approval");
    h.gate(&b, "plan_approval");
    assert_eq!(h.detail(&c).run.status, "queued", "only two runs execute at once");
    assert_eq!(h.client.calls("research"), 2);

    // Finishing one frees a slot for the queued run.
    h.decide(&a, &h.gate(&a, "plan_approval"), false);
    h.finished(&a, "rejected");
    h.gate(&c, "plan_approval");

    // Pause: approving the plan does not start the Developer until resumed.
    agent_pause(h.state(), b.clone()).unwrap();
    h.decide(&b, &h.gate(&b, "plan_approval"), true);
    h.wait(&b, "paused", |d| d.run.status == "paused");
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(h.client.calls("developer"), 0, "paused runs do no work");
    agent_resume(h.app.handle().clone(), h.state(), b.clone()).unwrap();
    h.gate(&b, "command");
    assert_eq!(h.client.calls("developer"), 1);

    assert!(delete_agent_run(h.state(), b.clone()).is_err(), "an active task cannot be deleted");
    for run in [&b, &c] {
        agent_cancel(h.app.handle().clone(), h.state(), run.to_string()).unwrap();
        h.finished(run, "cancelled");
    }
    delete_agent_run(h.state(), b.clone()).unwrap();
    assert!(get_agent_run(h.state(), b).is_err());
}

#[test]
fn creation_is_validated() {
    let h = Harness::new();
    let make = |project: &str, outcome: &str| {
        create_agent_run(h.app.handle().clone(), h.state(), project.into(), outcome.into(), "scripted".into(), None, None)
    };
    assert!(make("p", "short").is_err());
    assert!(make("missing", "A perfectly reasonable outcome").is_err());
    // Without the scripted client a real provider is required.
    *h.state().agents.client.lock().unwrap() = None;
    assert!(make("p", "A perfectly reasonable outcome").unwrap_err().to_string().contains("provider"));

    // A task is pinned to an explicitly chosen, enabled connection and model.
    let with = |provider: &str, model: Option<&str>| {
        create_agent_run(
            h.app.handle().clone(),
            h.state(),
            "p".into(),
            "A perfectly reasonable outcome".into(),
            provider.into(),
            model.map(String::from),
            None,
        )
    };
    let add = |id: &str, default_model: Option<&str>, enabled: i64| {
        h.state().db.conn().execute(
            "INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at, enabled) VALUES (?1, ?1, 'ollama', 'http://127.0.0.1:9', ?2, 0, '', ?3)",
            rusqlite::params![id, default_model, enabled],
        ).unwrap();
    };
    add("bare", None, 1);
    add("preset", Some("default:1"), 1);
    add("off", Some("default:1"), 0);
    assert!(with("bare", None).unwrap_err().to_string().contains("Choose a model"));
    assert!(with("bare", Some("   ")).unwrap_err().to_string().contains("Choose a model"));
    assert!(with("off", Some("m:1")).unwrap_err().to_string().contains("disabled"));
    let model_of = |id: &str| -> Option<String> {
        h.state().db.conn().query_row("SELECT model FROM agent_runs WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    };
    let explicit = with("bare", Some(" chosen:2 ")).unwrap();
    assert_eq!(model_of(&explicit.id).as_deref(), Some("chosen:2"));
    // The connection's default model is resolved once, at creation, and stored with the task,
    // so changing the default later cannot switch a task to another model.
    let pinned = with("preset", None).unwrap();
    assert_eq!(model_of(&pinned.id).as_deref(), Some("default:1"));
    h.state().db.conn().execute("UPDATE providers SET default_model = 'other:9' WHERE id = 'preset'", []).unwrap();
    assert_eq!(model_of(&pinned.id).as_deref(), Some("default:1"));
    // A model that is not on this machine needs an explicit yes, because the task sends it code.
    h.state().db.conn().execute(
        "INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at) VALUES ('far', 'far', 'openai', 'https://api.example.invalid/v1', 'big:1', 0, '')",
        [],
    ).unwrap();
    let remote = |allow: Option<bool>| {
        create_agent_run(h.app.handle().clone(), h.state(), "p".into(), "A perfectly reasonable outcome".into(), "far".into(), None, allow)
    };
    assert!(remote(None).unwrap_err().to_string().contains("not on this machine"));
    assert!(remote(Some(false)).unwrap_err().to_string().contains("not on this machine"));
    let allowed = remote(Some(true)).unwrap();
    for id in [explicit.id, pinned.id, allowed.id] {
        let _ = agent_cancel(h.app.handle().clone(), h.state(), id);
    }
}

#[test]
fn research_documentation_is_top_level_capped_and_never_follows_symlinks() {
    let dir = std::env::temp_dir().join(format!("nexus-docs-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("README.md"), "# Demo\nRun tests with `make test`.\n").unwrap();
    fs::write(dir.join("CONTRIBUTING.md"), "x".repeat(20_000)).unwrap();
    fs::write(dir.join("readme.txt"), "second readme\n").unwrap();
    fs::write(dir.join("notes.md"), "not documentation we look for\n").unwrap();
    fs::write(dir.join("sub/README.md"), "nested\n").unwrap();
    let outside = std::env::temp_dir().join(format!("nexus-secret-{}", uuid::Uuid::new_v4()));
    fs::write(&outside, "outside the project\n").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("README.rst")).unwrap();

    let docs = engine::project_docs(&dir);
    let names: Vec<&str> = docs.iter().map(|d| d.0.as_str()).collect();
    assert_eq!(names, ["README.md", "readme.txt"], "READMEs first, two files at most, no symlink, nothing nested");
    assert!(docs[0].1.contains("make test"));

    fs::remove_file(dir.join("readme.txt")).unwrap();
    let docs = engine::project_docs(&dir);
    assert_eq!(docs[1].0, "CONTRIBUTING.md");
    assert!(docs[1].1.len() < 6_100 && docs[1].1.contains("truncated"), "long documents are cut");

    fs::remove_dir_all(&dir).unwrap();
    fs::remove_file(&outside).unwrap();
}
