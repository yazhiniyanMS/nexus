//! How approved commands run: security level, environment, time limit, output and resource limits.
//!
//! Approval decides *whether* a command runs. This module decides *how much it can touch* when
//! it does. Three levels, each described to the user exactly as enforced on their machine:
//!
//! 1. **Trusted** — runs as the user, with their files and network. Only credential-like
//!    environment variables are removed. Not a sandbox.
//! 2. **Restricted** — a sandbox (see [`sandbox`]): writes only to the working folder, temporary
//!    folders and tool caches; credential folders and NEXUS's own data unreadable; keychain
//!    unreachable; allow-listed environment; network allowed.
//! 3. **Isolated** — Restricted without tool-cache writes or network (this machine only), with
//!    package-manager settings hidden too.
//!
//! Restricted and Isolated need a backend; where none exists they are refused, never quietly
//! run at a lower level.

pub mod env;
pub mod sandbox;

use crate::error::{AppError, Result};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::State;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Trusted,
    Restricted,
    Isolated,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Trusted => "Trusted",
            Level::Restricted => "Restricted",
            Level::Isolated => "Isolated",
        }
    }
}

/// Whether `level` can be enforced here, and if not, why.
pub fn available(level: Level) -> std::result::Result<(), String> {
    if level == Level::Trusted || sandbox::backend() != &sandbox::Backend::None {
        return Ok(());
    }
    Err(format!(
        "{} execution is not available on this system: it needs the macOS sandbox, or bubblewrap (bwrap) on Linux, and neither was \
         found. NEXUS does not run the command with less protection than you chose; pick Trusted in Settings → Execution, or install \
         bubblewrap.",
        level.label()
    ))
}

/// The exact protection a level gives on this machine, for the UI and for each command's record.
pub fn describe(level: Level) -> String {
    let backend = sandbox::backend();
    let linux = matches!(backend, sandbox::Backend::Bubblewrap(_));
    match (level, backend.name()) {
        (Level::Trusted, _) => "Trusted: runs as you, with access to your files and the network. Credential-like environment variables \
             (keys, tokens, passwords, credential URLs) are removed. Approval is the only safeguard; this is not a sandbox."
            .into(),
        (_, None) => available(level).unwrap_err(),
        (Level::Restricted, Some(name)) => format!(
            "Restricted ({name}): can write only to the working folder, temporary folders and tool caches; cannot read SSH, cloud, \
             Git or registry credential folders, NEXUS's own data{}; network allowed; environment limited to an allow-list plus the \
             variables you allow; written files limited in size.{}",
            if linux { " or the desktop keyring" } else { " or the keychain" },
            if linux { " Runs in its own process namespace and stops if NEXUS exits." } else { "" }
        ),
        (Level::Isolated, Some(name)) => format!(
            "Isolated ({name}): can write only to the working folder and temporary folders; cannot read credential folders, \
             package-manager settings or NEXUS's own data{}; {}; environment limited to an allow-list plus the variables you allow; \
             written files limited in size{}.",
            if linux { " or the desktop keyring" } else { " or the keychain" },
            if linux {
                "no network at all, not even to other programs on this computer (only its own loopback)"
            } else {
                "no network except this computer (localhost)"
            },
            if linux { "; memory limited; own process namespace, stopped if NEXUS exits" } else { " (memory cannot be limited on macOS)" }
        ),
    }
}

/// User-configurable execution settings, stored under the `exec` key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ExecSettings {
    /// Level for task steps (commands the user wrote or accepted from a plan).
    pub task_level: Level,
    /// Level for agent test commands.
    pub agent_level: Level,
    /// Default time limit for a task step, in seconds. A task can override it.
    pub task_timeout_secs: u64,
    /// Time limit for an agent test command, in seconds.
    pub agent_timeout_secs: u64,
    /// Output kept per command, in KB.
    pub output_limit_kb: u64,
    /// Variables passed even though the policy would remove them (for example `GITHUB_TOKEN`).
    pub allowed_env: Vec<String>,
    /// Largest file a Restricted or Isolated command may write, in MB.
    pub max_file_mb: u64,
    /// Memory limit for Isolated commands on Linux, in MB.
    pub memory_mb: u64,
    /// Agent tasks that run at the same time; the rest wait in the queue.
    pub max_concurrent_agents: u32,
    /// Agent tasks that may wait in the queue.
    pub max_queued_agents: u32,
    /// Task runs at the same time.
    pub max_running_tasks: u32,
}

pub const MIN_TIMEOUT_SECS: u64 = 5;
pub const MAX_TIMEOUT_SECS: u64 = 24 * 60 * 60;

impl Default for ExecSettings {
    fn default() -> Self {
        ExecSettings {
            task_level: Level::Trusted,
            // Agent commands come from model output and repository manifests; confine them where
            // the machine can.
            agent_level: if available(Level::Restricted).is_ok() { Level::Restricted } else { Level::Trusted },
            task_timeout_secs: 30 * 60,
            agent_timeout_secs: 15 * 60,
            output_limit_kb: 200,
            allowed_env: vec![],
            max_file_mb: 4096,
            memory_mb: 8192,
            max_concurrent_agents: 2,
            max_queued_agents: 20,
            max_running_tasks: 4,
        }
    }
}

impl ExecSettings {
    fn validate(&self) -> Result<()> {
        let secs = |v: u64, what: &str| -> Result<()> {
            if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&v) {
                return Err(AppError::Msg(format!("{what} must be between {MIN_TIMEOUT_SECS} seconds and 24 hours")));
            }
            Ok(())
        };
        secs(self.task_timeout_secs, "The task time limit")?;
        secs(self.agent_timeout_secs, "The agent command time limit")?;
        for (level, what) in [(self.task_level, "tasks"), (self.agent_level, "agent commands")] {
            available(level).map_err(|e| AppError::Msg(format!("For {what}: {e}")))?;
        }
        if !(16..=10_240).contains(&self.output_limit_kb) {
            return Err("Output kept per command must be between 16 KB and 10 MB".into());
        }
        if !(1..=1_048_576).contains(&self.max_file_mb) || !(256..=1_048_576).contains(&self.memory_mb) {
            return Err("File size limit must be 1 MB–1 TB and the memory limit 256 MB–1 TB".into());
        }
        if !(1..=8).contains(&self.max_concurrent_agents)
            || !(1..=200).contains(&self.max_queued_agents)
            || !(1..=32).contains(&self.max_running_tasks)
        {
            return Err("Concurrency limits: 1–8 agents at once, 1–200 queued, 1–32 task runs".into());
        }
        if self.allowed_env.len() > 64 {
            return Err("Allow at most 64 environment variables".into());
        }
        if let Some(bad) = self.allowed_env.iter().find(|n| !env::valid_name(n)) {
            return Err(AppError::Msg(format!("“{bad}” is not a valid environment variable name")));
        }
        Ok(())
    }
}

const KEY: &str = "exec";

/// The stored settings. Anything stored that is no longer valid (a level whose backend went
/// away, for example) falls back to the default for that field.
pub fn load(conn: &rusqlite::Connection) -> ExecSettings {
    let mut s: ExecSettings = crate::settings::get(conn, KEY).unwrap_or_default();
    let d = ExecSettings::default();
    if available(s.task_level).is_err() {
        s.task_level = d.task_level;
    }
    if available(s.agent_level).is_err() {
        s.agent_level = d.agent_level;
    }
    if s.validate().is_err() {
        // Keep the user's levels and allowed variables; reset numbers that are out of range.
        let keep = (s.task_level, s.agent_level, s.allowed_env.clone());
        s = ExecSettings { task_level: keep.0, agent_level: keep.1, allowed_env: keep.2, ..d };
        s.allowed_env.retain(|n| env::valid_name(n));
        s.allowed_env.truncate(64);
    }
    s
}

/// How one command runs.
#[derive(Debug, Clone)]
pub struct Policy {
    pub level: Level,
    pub timeout: Duration,
    pub output_limit: usize,
    pub allowed_env: Vec<String>,
    /// Enforced with resource limits on Unix; Windows has no equivalent without a job object.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub max_file_bytes: u64,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub memory_bytes: u64,
    /// NEXUS's data folder, hidden from Restricted and Isolated commands.
    pub data_dir: Option<PathBuf>,
}

impl Policy {
    fn from(s: &ExecSettings, level: Level, timeout_secs: u64, data_dir: Option<&Path>) -> Policy {
        Policy {
            level,
            timeout: Duration::from_secs(timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)),
            output_limit: (s.output_limit_kb * 1024) as usize,
            allowed_env: s.allowed_env.clone(),
            max_file_bytes: s.max_file_mb * 1024 * 1024,
            memory_bytes: s.memory_mb * 1024 * 1024,
            data_dir: data_dir.map(Path::to_path_buf),
        }
    }

    /// A task step: the task's own time limit if it has one, else the default.
    pub fn for_task(s: &ExecSettings, task_timeout_secs: Option<u64>, data_dir: Option<&Path>) -> Policy {
        Policy::from(s, s.task_level, task_timeout_secs.unwrap_or(s.task_timeout_secs), data_dir)
    }

    pub fn for_agent(s: &ExecSettings, data_dir: Option<&Path>) -> Policy {
        Policy::from(s, s.agent_level, s.agent_timeout_secs, data_dir)
    }
}

pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        s if s % 3600 == 0 => format!("{} h", s / 3600),
        s if s >= 60 && s % 60 == 0 => format!("{} min", s / 60),
        s if s >= 60 => format!("{} min {} s", s / 60, s % 60),
        s => format!("{s} s"),
    }
}

#[derive(Serialize)]
pub struct LevelInfo {
    level: Level,
    available: bool,
    description: String,
}

#[derive(Serialize)]
pub struct Capabilities {
    /// The isolation backend found on this machine, if any.
    backend: Option<&'static str>,
    os: &'static str,
    levels: Vec<LevelInfo>,
}

#[tauri::command]
pub fn exec_capabilities() -> Capabilities {
    Capabilities {
        backend: sandbox::backend().name(),
        os: std::env::consts::OS,
        levels: [Level::Trusted, Level::Restricted, Level::Isolated]
            .into_iter()
            .map(|level| LevelInfo { level, available: available(level).is_ok(), description: describe(level) })
            .collect(),
    }
}

#[tauri::command]
pub fn exec_settings_get(state: State<'_, AppState>) -> ExecSettings {
    load(&state.db.conn())
}

#[tauri::command]
pub fn exec_settings_set(state: State<'_, AppState>, settings: ExecSettings) -> Result<ExecSettings> {
    let mut s = settings;
    s.allowed_env = s.allowed_env.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
    s.allowed_env.dedup();
    s.validate()?;
    let conn = state.db.conn();
    crate::settings::set(&conn, KEY, &s)?;
    crate::activity::log(
        &conn,
        "settings",
        &format!("Execution settings changed: tasks {}, agent commands {}", s.task_level.label(), s.agent_level.label()),
        None,
    );
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_validated_and_bad_stored_values_fall_back() {
        let ok = ExecSettings::default();
        assert!(ok.validate().is_ok());
        for bad in [
            ExecSettings { task_timeout_secs: 1, ..ok.clone() },
            ExecSettings { agent_timeout_secs: MAX_TIMEOUT_SECS + 1, ..ok.clone() },
            ExecSettings { output_limit_kb: 1, ..ok.clone() },
            ExecSettings { allowed_env: vec!["NOT VALID".into()], ..ok.clone() },
            ExecSettings { max_concurrent_agents: 0, ..ok.clone() },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        let db = crate::db::Db::in_memory().unwrap();
        crate::settings::set(&db.conn(), KEY, &serde_json::json!({"task_timeout_secs": 0, "allowed_env": ["GITHUB_TOKEN", "a b"]}))
            .unwrap();
        let loaded = load(&db.conn());
        assert_eq!(loaded.task_timeout_secs, ExecSettings::default().task_timeout_secs);
        assert_eq!(loaded.allowed_env, vec!["GITHUB_TOKEN".to_string()]);
    }

    #[test]
    fn unavailable_levels_are_refused_not_downgraded() {
        match sandbox::backend() {
            sandbox::Backend::None => {
                assert!(available(Level::Restricted).is_err());
                let s = ExecSettings { task_level: Level::Isolated, ..ExecSettings::default() };
                assert!(s.validate().unwrap_err().to_string().contains("not available"));
                assert_eq!(ExecSettings::default().agent_level, Level::Trusted);
            }
            _ => {
                assert!(available(Level::Isolated).is_ok());
                assert_eq!(ExecSettings::default().agent_level, Level::Restricted);
            }
        }
        assert!(describe(Level::Trusted).contains("not a sandbox"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_duration(Duration::from_secs(900)), "15 min");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1 h");
        assert_eq!(format_duration(Duration::from_secs(90)), "1 min 30 s");
        assert_eq!(format_duration(Duration::from_secs(5)), "5 s");
    }
}
