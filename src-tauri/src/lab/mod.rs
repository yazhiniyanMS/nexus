//! Model Lab: a local-first workbench for models.
//!
//! - `registry`  the model library: what connected runtimes and local files report
//! - `runtime`   detecting runtimes, their health, and processes NEXUS starts itself
//! - `storage`   the folder NEXUS downloads model files into
//! - `download`  model downloads (files over HTTPS, pulls through Ollama) and the Hugging Face listing
//! - `infer`     streaming inference with measured timings, and saved playground sessions
//! - `bench`     repeatable benchmarks, their history and export
//! - `gguf`      reading model-file headers
//!
//! Providers and credentials stay in `ai`; this module never sees an API key.
//! Nothing here runs a shell command, and model output is only ever treated as text.

pub mod bench;
pub mod download;
pub mod gguf;
pub mod hardware;
pub mod infer;
pub mod registry;
pub mod runtime;
pub mod storage;
#[cfg(test)]
pub mod testserver;

use crate::error::{AppError, Result};
use crate::{ai, db, settings, AppState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;
use tokio::sync::watch;

/// Cancel handles of running jobs of one kind, by job id.
#[derive(Default)]
pub struct Cancels(Mutex<HashMap<String, watch::Sender<bool>>>);

impl Cancels {
    /// Register a job. Fails when a job with this id is already running.
    pub fn register(&self, id: &str) -> Result<watch::Receiver<bool>> {
        let mut jobs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if jobs.contains_key(id) {
            return Err("This is already running".into());
        }
        let (tx, rx) = watch::channel(false);
        jobs.insert(id.to_string(), tx);
        Ok(rx)
    }

    /// Ask a job to stop. Returns false when no such job is running.
    pub fn cancel(&self, id: &str) -> bool {
        match self.0.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            Some(tx) => {
                let _ = tx.send(true);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self, id: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    pub fn cancel_all(&self) {
        for tx in self.0.lock().unwrap_or_else(|e| e.into_inner()).values() {
            let _ = tx.send(true);
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
    }

    pub fn count(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// True once `cancel` has been signalled.
pub(crate) fn cancelled(cancel: &watch::Receiver<bool>) -> bool {
    *cancel.borrow()
}

/// Splits a byte stream that arrives in arbitrary pieces into complete text lines.
#[derive(Default)]
pub(crate) struct Lines {
    buf: Vec<u8>,
}

impl Lines {
    /// Add a piece and take every line it completes (without the line ending).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = vec![];
        while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=end).collect();
            out.push(String::from_utf8_lossy(&line[..line.len() - 1]).trim_end_matches('\r').to_string());
        }
        out
    }

    /// Whatever is left when the stream ends without a final line ending.
    pub fn finish(self) -> Option<String> {
        let rest = String::from_utf8_lossy(&self.buf).trim().to_string();
        (!rest.is_empty()).then_some(rest)
    }
}

#[derive(Default)]
pub struct Lab {
    pub(crate) infer: Cancels,
    pub(crate) downloads: Cancels,
    pub(crate) bench: Cancels,
    pub(crate) runtimes: runtime::Managed,
}

/// Generation settings. Every field is optional: an unset field is not sent, so the runtime's
/// own default applies. Fields a runtime does not support are never sent to it.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct GenParams {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<i64>,
    pub repeat_penalty: Option<f64>,
    /// Ollama: false asks a thinking-capable model to answer without thinking.
    pub think: Option<bool>,
}

impl GenParams {
    pub fn validate(&self) -> Result<()> {
        let range = |name: &str, v: Option<f64>, lo: f64, hi: f64| match v {
            Some(v) if !v.is_finite() || v < lo || v > hi => Err(AppError::Msg(format!("{name} must be between {lo} and {hi}"))),
            _ => Ok(()),
        };
        range("Temperature", self.temperature, 0.0, 2.0)?;
        range("Top-p", self.top_p, 0.0, 1.0)?;
        range("Repeat penalty", self.repeat_penalty, 0.0, 3.0)?;
        if self.top_k.is_some_and(|k| k > 1000) {
            return Err("Top-k must be 1000 or less".into());
        }
        if self.max_tokens.is_some_and(|m| m == 0 || m > 200_000) {
            return Err("Maximum output tokens must be between 1 and 200000".into());
        }
        Ok(())
    }

    /// Keep only the settings the given kind of runtime accepts.
    pub fn for_kind(&self, kind: &str) -> GenParams {
        let supports = |name: &str| kind_info(kind).is_some_and(|k| k.params.contains(&name));
        GenParams {
            temperature: self.temperature.filter(|_| supports("temperature")),
            top_p: self.top_p.filter(|_| supports("top_p")),
            top_k: self.top_k.filter(|_| supports("top_k")),
            max_tokens: self.max_tokens.filter(|_| supports("max_tokens")),
            seed: self.seed.filter(|_| supports("seed")),
            repeat_penalty: self.repeat_penalty.filter(|_| supports("repeat_penalty")),
            think: self.think.filter(|_| supports("think")),
        }
    }
}

/// What NEXUS knows about one kind of connection.
#[derive(Serialize, Clone, Debug)]
pub struct KindInfo {
    pub kind: &'static str,
    pub label: &'static str,
    pub default_url: &'static str,
    pub needs_key: bool,
    /// Generation settings this kind accepts.
    pub params: &'static [&'static str],
    pub summary: &'static str,
}

const KINDS: [KindInfo; 4] = [
    KindInfo {
        kind: "ollama",
        label: "Ollama",
        default_url: "http://localhost:11434",
        needs_key: false,
        params: &["temperature", "top_p", "top_k", "max_tokens", "seed", "repeat_penalty", "think"],
        summary: "Ollama's own API. Reports model details, load time, token counts and memory use.",
    },
    KindInfo {
        kind: "llamacpp",
        label: "llama.cpp server",
        default_url: "http://localhost:8080",
        needs_key: false,
        params: &["temperature", "top_p", "top_k", "max_tokens", "seed", "repeat_penalty"],
        summary: "A llama-server process serving one GGUF model. Reports context size and timings.",
    },
    KindInfo {
        kind: "openai",
        label: "OpenAI-compatible",
        default_url: "http://localhost:1234/v1",
        needs_key: false,
        params: &["temperature", "top_p", "max_tokens", "seed"],
        summary: "Any endpoint that speaks the OpenAI chat API (LM Studio, vLLM, hosted services). Lists model names only.",
    },
    KindInfo {
        kind: "anthropic",
        label: "Anthropic",
        default_url: "https://api.anthropic.com",
        needs_key: true,
        // Current Claude models reject temperature, top-p and top-k, so they are not offered.
        params: &["max_tokens"],
        summary: "The Claude API. Remote: requests leave this machine. Needs an API key.",
    },
];

pub fn kind_info(kind: &str) -> Option<&'static KindInfo> {
    KINDS.iter().find(|k| k.kind == kind)
}

#[tauri::command]
pub fn lab_kinds() -> Vec<KindInfo> {
    KINDS.to_vec()
}

const SETTINGS_KEY: &str = "model_lab";

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct LabSettings {
    pub default_provider_id: Option<String>,
    pub default_model: Option<String>,
    /// Folder for model files NEXUS downloads. None means the default inside the app's data folder.
    /// Changed only through the folder picker opened by the core.
    pub storage_dir: Option<String>,
    pub params: GenParams,
}

/// Current settings, with a default that points at a deleted connection cleared.
pub fn load_settings(state: &AppState) -> Result<LabSettings> {
    let mut s: LabSettings = settings::get(&state.db.conn(), SETTINGS_KEY)?;
    if let Some(id) = &s.default_provider_id {
        if ai::get_provider(state, id).is_err() {
            s.default_provider_id = None;
            s.default_model = None;
        }
    }
    Ok(s)
}

pub(crate) fn save_settings(state: &AppState, s: &LabSettings) -> Result<()> {
    settings::set(&state.db.conn(), SETTINGS_KEY, s)
}

#[tauri::command]
pub fn lab_settings_get(state: State<'_, AppState>) -> Result<LabSettings> {
    load_settings(&state)
}

/// Save the default connection, model and generation settings. The storage folder is not
/// settable here; see `storage::lab_storage_choose`.
#[tauri::command]
pub fn lab_settings_set(
    state: State<'_, AppState>,
    default_provider_id: Option<String>,
    default_model: Option<String>,
    params: GenParams,
) -> Result<LabSettings> {
    params.validate()?;
    let default_provider_id = default_provider_id.filter(|id| !id.trim().is_empty());
    if let Some(id) = &default_provider_id {
        ai::get_provider(&state, id)?;
    }
    let default_model = default_model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty() && default_provider_id.is_some());
    if default_model.as_ref().is_some_and(|m| m.chars().count() > 200) {
        return Err("Model name is too long".into());
    }
    let mut s = load_settings(&state)?;
    s.default_provider_id = default_provider_id;
    s.default_model = default_model;
    s.params = params;
    save_settings(&state, &s)?;
    Ok(s)
}

/// Called once at startup: nothing from the previous run is still in progress.
pub fn recover_interrupted(db: &db::Db) -> Result<()> {
    let conn = db.conn();
    let now = db::now();
    conn.execute(
        "UPDATE lab_downloads SET status = 'interrupted', error = 'NEXUS closed during this download. Resume to continue.', updated_at = ?1
         WHERE status IN ('queued', 'downloading', 'verifying')",
        [&now],
    )?;
    conn.execute(
        "UPDATE lab_bench_suites SET status = 'interrupted', error = 'NEXUS closed while this benchmark was running.', finished_at = ?1
         WHERE status = 'running'",
        [&now],
    )?;
    Ok(())
}

/// Called when the app exits: stop everything this module started.
pub fn shutdown(state: &AppState) {
    state.lab.infer.cancel_all();
    state.lab.downloads.cancel_all();
    state.lab.bench.cancel_all();
    state.lab.runtimes.stop_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_settings_are_validated_and_filtered_per_runtime() {
        let all = GenParams {
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            max_tokens: Some(256),
            seed: Some(7),
            repeat_penalty: Some(1.1),
            think: Some(false),
        };
        all.validate().unwrap();
        assert_eq!(all.for_kind("ollama"), all);
        let openai = all.for_kind("openai");
        assert_eq!((openai.top_k, openai.repeat_penalty, openai.think), (None, None, None));
        assert_eq!((openai.temperature, openai.seed), (Some(0.2), Some(7)));
        let claude = all.for_kind("anthropic");
        assert_eq!(claude, GenParams { max_tokens: Some(256), ..Default::default() }, "sampling settings are never sent to Claude");
        assert_eq!(all.for_kind("unknown"), GenParams::default());

        for bad in [
            GenParams { temperature: Some(2.5), ..Default::default() },
            GenParams { temperature: Some(f64::NAN), ..Default::default() },
            GenParams { top_p: Some(-0.1), ..Default::default() },
            GenParams { top_k: Some(5000), ..Default::default() },
            GenParams { max_tokens: Some(0), ..Default::default() },
            GenParams { repeat_penalty: Some(9.0), ..Default::default() },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn lines_are_reassembled_across_arbitrary_chunks() {
        let mut lines = Lines::default();
        assert_eq!(lines.push(b"{\"a\":"), Vec::<String>::new());
        assert_eq!(lines.push(b"1}\r\n{\"b\":2}\n\nta"), ["{\"a\":1}", "{\"b\":2}", ""]);
        // A multi-byte character split between chunks survives.
        let text = "é\n".as_bytes();
        assert!(lines.push(&text[..1]).is_empty());
        assert_eq!(lines.push(&text[1..]), ["taé"]);
        assert_eq!(lines.push(b"tail"), Vec::<String>::new());
        assert_eq!(lines.finish().as_deref(), Some("tail"));
        assert_eq!(Lines::default().finish(), None);
    }

    #[test]
    fn cancel_handles_track_running_jobs() {
        let jobs = Cancels::default();
        let rx = jobs.register("a").unwrap();
        assert!(jobs.register("a").is_err(), "the same job cannot run twice");
        assert!(!cancelled(&rx));
        assert!(jobs.cancel("a"));
        assert!(cancelled(&rx));
        assert!(!jobs.cancel("missing"));
        jobs.finish("a");
        assert_eq!(jobs.count(), 0);
        let (b, c) = (jobs.register("b").unwrap(), jobs.register("c").unwrap());
        jobs.cancel_all();
        assert!(cancelled(&b) && cancelled(&c));
    }

    #[test]
    fn shutting_down_stops_everything_the_lab_started() {
        let state = AppState::for_tests();
        let (a, b, c) = (
            state.lab.infer.register("request").unwrap(),
            state.lab.downloads.register("download").unwrap(),
            state.lab.bench.register("suite").unwrap(),
        );
        assert!(!cancelled(&a) && !cancelled(&b) && !cancelled(&c));
        shutdown(&state);
        assert!(cancelled(&a) && cancelled(&b) && cancelled(&c), "requests, downloads and benchmarks are all told to stop");
        // The next start then reports what was cut short (see the interrupted test below).
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn settings_persist_and_forget_a_deleted_default_connection() {
        let state = AppState::for_tests();
        assert_eq!(load_settings(&state).unwrap(), LabSettings::default());
        state.db.conn().execute("INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES ('p', 'Local', 'ollama', 'http://localhost:11434', 0, '')", []).unwrap();
        let mut s = LabSettings { default_provider_id: Some("p".into()), default_model: Some("m:1".into()), ..Default::default() };
        s.params.temperature = Some(0.0);
        save_settings(&state, &s).unwrap();
        assert_eq!(load_settings(&state).unwrap(), s);
        state.db.conn().execute("DELETE FROM providers WHERE id = 'p'", []).unwrap();
        let after = load_settings(&state).unwrap();
        assert_eq!((after.default_provider_id, after.default_model), (None, None));
        assert_eq!(after.params.temperature, Some(0.0), "other settings survive");
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn startup_marks_unfinished_downloads_and_benchmarks_as_interrupted() {
        let db = db::Db::in_memory().unwrap();
        {
            let conn = db.conn();
            for (id, status) in [("a", "downloading"), ("b", "completed"), ("c", "verifying")] {
                conn.execute("INSERT INTO lab_downloads (id, kind, source, name, status, created_at, updated_at) VALUES (?1, 'file', 'https://x/y.gguf', 'y.gguf', ?2, '', '')", [id, status]).unwrap();
            }
            for (id, status) in [("s1", "running"), ("s2", "completed")] {
                conn.execute(
                    "INSERT INTO lab_bench_suites (id, name, config, hardware, status, created_at) VALUES (?1, 'n', '{}', '{}', ?2, '')",
                    [id, status],
                )
                .unwrap();
            }
        }
        recover_interrupted(&db).unwrap();
        let conn = db.conn();
        let status = |table: &str, id: &str| -> String {
            conn.query_row(&format!("SELECT status FROM {table} WHERE id = ?1"), [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(
            (status("lab_downloads", "a"), status("lab_downloads", "b"), status("lab_downloads", "c")),
            ("interrupted".into(), "completed".into(), "interrupted".into())
        );
        assert_eq!((status("lab_bench_suites", "s1"), status("lab_bench_suites", "s2")), ("interrupted".into(), "completed".into()));
    }
}
