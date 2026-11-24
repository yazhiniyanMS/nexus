//! Benchmarks: the same prompts, with the same settings, run several times against each chosen
//! model, one request at a time. Every figure is either measured here (time to first token,
//! total time) or reported by the runtime (token counts, load and prompt-processing time,
//! memory). Nothing is scored, weighted or estimated.
//!
//! A suite stores what it tested (connection, runtime, model details as known at the time)
//! and the machine it ran on, so results are never read without their conditions.

use super::hardware::{hardware, Hardware};
use super::infer::{self, InferRequest, Metrics};
use super::{cancelled, registry, runtime, GenParams};
use crate::ai;
use crate::error::{AppError, Result};
use crate::{activity, db, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::watch;

pub const EVENT_BENCH: &str = "nexus://lab-bench";

const MAX_TARGETS: usize = 8;
const MAX_PROMPTS: usize = 10;
const MAX_REPEATS: u32 = 20;
const MAX_RUNS: usize = 400;
const MAX_OUTPUT_CHARS: usize = 4000;
/// After this many failures in a row a model is skipped; it is not going to start working.
const GIVE_UP_AFTER: usize = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Target {
    pub provider_id: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    pub label: String,
    pub text: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BenchInput {
    pub name: Option<String>,
    pub targets: Vec<Target>,
    pub prompts: Vec<Prompt>,
    pub system: Option<String>,
    #[serde(default)]
    pub params: GenParams,
    pub repeats: u32,
    /// Run the first prompt once per model before measuring, so loading does not skew the runs.
    #[serde(default)]
    pub warmup: bool,
    /// Ollama only: unload the model first, so its load time is measured from cold.
    #[serde(default)]
    pub cold_start: bool,
    /// Must be true when any target is not on this machine.
    #[serde(default)]
    pub allow_remote: bool,
}

/// One tested model, described as it was known when the benchmark ran.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct TargetInfo {
    pub provider_id: String,
    pub provider_name: String,
    /// Connection kind: ollama, llamacpp, openai or anthropic.
    pub runtime: String,
    pub runtime_version: Option<String>,
    /// Host and port the requests went to.
    pub endpoint: String,
    pub local: bool,
    pub model: String,
    pub quantization: Option<String>,
    pub parameters: Option<String>,
    pub context_length: Option<u64>,
    pub format: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SuiteConfig {
    pub targets: Vec<TargetInfo>,
    pub prompts: Vec<Prompt>,
    pub system: Option<String>,
    pub params: GenParams,
    pub repeats: u32,
    pub warmup: bool,
    pub cold_start: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Run {
    pub id: i64,
    pub target: usize,
    pub prompt: usize,
    /// 0 for the warm-up, then 1..=repeats.
    pub iteration: u32,
    pub warmup: bool,
    /// "ok", "error" or "cancelled".
    pub status: String,
    pub error: Option<String>,
    pub metrics: Metrics,
    pub memory_bytes: Option<u64>,
    pub memory_vram_bytes: Option<u64>,
    pub output: String,
    pub at: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Stat {
    pub n: usize,
    pub mean: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    /// Sample standard deviation; 0 for a single run.
    pub stddev: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct TargetSummary {
    pub target: usize,
    /// Measured runs that succeeded (the warm-up is not counted).
    pub runs: usize,
    pub errors: usize,
    pub ttft_ms: Option<Stat>,
    pub total_ms: Option<Stat>,
    pub tokens_per_second: Option<Stat>,
    /// "runtime", "derived" or "mixed", describing where tokens per second came from.
    pub tps_source: Option<String>,
    pub prompt_eval_ms: Option<Stat>,
    pub input_tokens: Option<Stat>,
    pub output_tokens: Option<Stat>,
    /// Load time the runtime reported for this model's first request in the suite.
    pub load_ms: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub memory_vram_bytes: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Suite {
    pub id: String,
    pub name: String,
    pub config: SuiteConfig,
    pub hardware: Hardware,
    /// "running", "completed", "cancelled", "failed" or "interrupted".
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub planned_runs: usize,
    pub runs: Vec<Run>,
    pub summary: Vec<TargetSummary>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SuiteSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub models: Vec<String>,
    pub prompts: usize,
    pub repeats: u32,
    pub runs: usize,
}

// ------------------------------------------------------------------ statistics

pub fn stat(values: &[f64]) -> Option<Stat> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let median = if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 };
    let stddev = if n > 1 { (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() } else { 0.0 };
    Some(Stat { n, mean, median, min: v[0], max: v[n - 1], stddev })
}

pub fn planned_runs(config: &SuiteConfig) -> usize {
    config.targets.len() * (config.prompts.len() * config.repeats as usize + usize::from(config.warmup))
}

pub fn summarize(config: &SuiteConfig, runs: &[Run]) -> Vec<TargetSummary> {
    (0..config.targets.len())
        .map(|target| {
            let of_target: Vec<&Run> = runs.iter().filter(|r| r.target == target).collect();
            let measured: Vec<&Run> = of_target.iter().copied().filter(|r| !r.warmup && r.status == "ok").collect();
            let collect =
                |pick: &dyn Fn(&Metrics) -> Option<f64>| stat(&measured.iter().filter_map(|r| pick(&r.metrics)).collect::<Vec<_>>());
            let sources: Vec<&str> = measured.iter().filter_map(|r| r.metrics.tps_source.as_deref()).collect();
            let tps_source = match (sources.contains(&"runtime"), sources.contains(&"derived")) {
                (true, true) => Some("mixed".to_string()),
                (true, false) => Some("runtime".to_string()),
                (false, true) => Some("derived".to_string()),
                (false, false) => None,
            };
            TargetSummary {
                target,
                runs: measured.len(),
                errors: of_target.iter().filter(|r| r.status == "error").count(),
                ttft_ms: collect(&|m| m.ttft_ms),
                total_ms: collect(&|m| Some(m.total_ms)),
                tokens_per_second: collect(&|m| m.tokens_per_second),
                tps_source,
                prompt_eval_ms: collect(&|m| m.prompt_eval_ms),
                input_tokens: collect(&|m| m.input_tokens.map(|t| t as f64)),
                output_tokens: collect(&|m| m.output_tokens.map(|t| t as f64)),
                // The first request is the one that had to load the model, if any did.
                load_ms: of_target.iter().filter(|r| r.status == "ok").find_map(|r| r.metrics.load_ms),
                memory_bytes: of_target.iter().rev().find_map(|r| r.memory_bytes),
                memory_vram_bytes: of_target.iter().rev().find_map(|r| r.memory_vram_bytes),
            }
        })
        .collect()
}

// ------------------------------------------------------------------ storage

fn load_runs(state: &AppState, suite_id: &str) -> Result<Vec<Run>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, target, prompt, iteration, warmup, status, error, metrics, memory_bytes, memory_vram_bytes, output, at
         FROM lab_bench_runs WHERE suite_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([suite_id], |r| {
        Ok(Run {
            id: r.get(0)?,
            target: r.get::<_, i64>(1)? as usize,
            prompt: r.get::<_, i64>(2)? as usize,
            iteration: r.get::<_, i64>(3)? as u32,
            warmup: r.get::<_, i64>(4)? != 0,
            status: r.get(5)?,
            error: r.get(6)?,
            metrics: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
            memory_bytes: r.get::<_, Option<i64>>(8)?.map(|v| v as u64),
            memory_vram_bytes: r.get::<_, Option<i64>>(9)?.map(|v| v as u64),
            output: r.get(10)?,
            at: r.get(11)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn load_suite(state: &AppState, id: &str) -> Result<Suite> {
    #[allow(clippy::type_complexity)] // one row of columns, destructured right below
    let head: Option<(String, String, String, String, Option<String>, String, Option<String>)> = state
        .db
        .conn()
        .query_row("SELECT name, config, hardware, status, error, created_at, finished_at FROM lab_bench_suites WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })
        .optional()?;
    let (name, config, hw, status, error, created_at, finished_at) = head.ok_or("That benchmark no longer exists")?;
    let config: SuiteConfig = serde_json::from_str(&config).unwrap_or_default();
    let runs = load_runs(state, id)?;
    Ok(Suite {
        id: id.to_string(),
        name,
        hardware: serde_json::from_str(&hw).unwrap_or_default(),
        status,
        error,
        created_at,
        finished_at,
        planned_runs: planned_runs(&config),
        summary: summarize(&config, &runs),
        config,
        runs,
    })
}

#[allow(clippy::too_many_arguments)] // one column per argument; a struct would only rename them
fn record(
    state: &AppState,
    suite_id: &str,
    target: usize,
    prompt: usize,
    iteration: u32,
    warmup: bool,
    status: &str,
    error: Option<&str>,
    metrics: &Metrics,
    output: &str,
) -> Result<()> {
    let output: String = output.chars().take(MAX_OUTPUT_CHARS).collect();
    state.db.conn().execute(
        "INSERT INTO lab_bench_runs (suite_id, target, prompt, iteration, warmup, status, error, metrics, output, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            suite_id,
            target as i64,
            prompt as i64,
            iteration,
            warmup as i64,
            status,
            error,
            serde_json::to_string(metrics)?,
            output,
            db::now()
        ],
    )?;
    Ok(())
}

// ------------------------------------------------------------------ running

fn endpoint_of(base_url: &str) -> String {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|u| {
            Some(match u.port() {
                Some(port) => format!("{}:{port}", u.host_str()?),
                None => u.host_str()?.to_string(),
            })
        })
        .unwrap_or_else(|| base_url.to_string())
}

/// Check a request and describe what will be tested. Nothing is contacted except each
/// runtime's version endpoint.
pub async fn prepare(state: &AppState, input: &BenchInput) -> Result<(String, SuiteConfig)> {
    if input.targets.is_empty() || input.targets.len() > MAX_TARGETS {
        return Err(AppError::Msg(format!("Choose between 1 and {MAX_TARGETS} models")));
    }
    if input.prompts.is_empty() || input.prompts.len() > MAX_PROMPTS {
        return Err(AppError::Msg(format!("Use between 1 and {MAX_PROMPTS} prompts")));
    }
    if !(1..=MAX_REPEATS).contains(&input.repeats) {
        return Err(AppError::Msg(format!("Repeat each prompt between 1 and {MAX_REPEATS} times")));
    }
    input.params.validate()?;
    let mut prompts = vec![];
    for (i, p) in input.prompts.iter().enumerate() {
        let text = p.text.trim();
        if text.is_empty() || text.len() > 200_000 {
            return Err(AppError::Msg(format!("Prompt {} is empty or too long", i + 1)));
        }
        let label: String = p.label.trim().chars().take(60).collect();
        prompts.push(Prompt { label: if label.is_empty() { format!("Prompt {}", i + 1) } else { label }, text: text.to_string() });
    }
    let mut targets: Vec<TargetInfo> = vec![];
    for t in &input.targets {
        let model = t.model.trim();
        if model.is_empty() || model.chars().count() > 200 {
            return Err("Every benchmark target needs a model".into());
        }
        if targets.iter().any(|x| x.provider_id == t.provider_id && x.model == model) {
            return Err(AppError::Msg(format!("{model} is listed twice")));
        }
        let p = ai::usable_provider(state, &t.provider_id)?;
        if !p.local && !input.allow_remote {
            return Err(AppError::Msg(format!(
                "“{}” is not on this machine. Confirm sending the benchmark prompts to {} first.",
                p.name,
                endpoint_of(&p.base_url)
            )));
        }
        let meta = registry::lookup(state, &p.id, model);
        let parameters = meta.as_ref().and_then(|m| {
            m.parameter_label.clone().or_else(|| {
                m.parameter_count.map(|n| {
                    if n >= 1_000_000_000 {
                        format!("{:.1}B", n as f64 / 1e9)
                    } else {
                        format!("{:.0}M", n as f64 / 1e6)
                    }
                })
            })
        });
        targets.push(TargetInfo {
            provider_id: p.id.clone(),
            provider_name: p.name.clone(),
            runtime: p.kind.clone(),
            runtime_version: if p.kind == "ollama" { ai::test_connection(state, &p).await.version } else { None },
            endpoint: endpoint_of(&p.base_url),
            local: p.local,
            model: model.to_string(),
            quantization: meta.as_ref().and_then(|m| m.quantization.clone()),
            parameters,
            context_length: meta.as_ref().and_then(|m| m.context_length),
            format: meta.as_ref().and_then(|m| m.format.clone()),
        });
    }
    let config = SuiteConfig {
        targets,
        prompts,
        system: input.system.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from),
        params: input.params.clone(),
        repeats: input.repeats,
        warmup: input.warmup,
        cold_start: input.cold_start,
    };
    if planned_runs(&config) > MAX_RUNS {
        return Err(AppError::Msg(format!("That is {} requests; keep a benchmark to {MAX_RUNS} or fewer", planned_runs(&config))));
    }
    let name: String =
        input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(|n| n.chars().take(80).collect()).unwrap_or_else(|| {
            let models: Vec<&str> = config.targets.iter().map(|t| t.model.as_str()).collect();
            format!("{} · {}", models.join(" vs "), chrono::Local::now().format("%Y-%m-%d %H:%M"))
        });
    Ok((name, config))
}

pub fn create(state: &AppState, name: &str, config: &SuiteConfig) -> Result<String> {
    let id = db::new_id();
    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO lab_bench_suites (id, name, config, hardware, status, created_at) VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
        params![id, name, serde_json::to_string(config)?, serde_json::to_string(&hardware())?, db::now()],
    )?;
    activity::log(&conn, "models", &format!("Started benchmark “{name}”"), None);
    Ok(id)
}

/// Run a created suite to its end. `notify` is called after every recorded run.
pub async fn run_suite(
    state: &AppState,
    id: &str,
    mut cancel: watch::Receiver<bool>,
    allow_remote: bool,
    notify: &(dyn Fn() + Send + Sync),
) -> Result<()> {
    let suite = load_suite(state, id)?;
    let config = suite.config;
    let mut any_ok = false;
    let mut stopped = false;
    'targets: for (ti, target) in config.targets.iter().enumerate() {
        if cancelled(&cancel) {
            stopped = true;
            break;
        }
        let provider = ai::usable_provider(state, &target.provider_id);
        if config.cold_start && target.runtime == "ollama" {
            if let Ok(p) = &provider {
                let _ = runtime::unload(state, p, &target.model).await;
            }
        }
        // (prompt index, iteration, is warm-up)
        let mut plan: Vec<(usize, u32, bool)> = vec![];
        if config.warmup {
            plan.push((0, 0, true));
        }
        for pi in 0..config.prompts.len() {
            for i in 1..=config.repeats {
                plan.push((pi, i, false));
            }
        }
        let mut failures_in_a_row = 0;
        for (pi, iteration, warmup) in plan {
            if cancelled(&cancel) {
                stopped = true;
                break 'targets;
            }
            let request = InferRequest {
                id: db::new_id(),
                provider_id: target.provider_id.clone(),
                model: target.model.clone(),
                system: config.system.clone(),
                messages: vec![ai::ChatMessage { role: "user".into(), content: config.prompts[pi].text.clone() }],
                params: config.params.clone(),
                allow_remote,
            };
            match infer::run(state, &request, &mut cancel, &mut |_| {}).await {
                Ok(out) if out.cancelled => {
                    record(state, id, ti, pi, iteration, warmup, "cancelled", None, &out.metrics, &out.text)?;
                    notify();
                    stopped = true;
                    break 'targets;
                }
                Ok(out) => {
                    record(state, id, ti, pi, iteration, warmup, "ok", None, &out.metrics, &out.text)?;
                    any_ok = true;
                    failures_in_a_row = 0;
                }
                Err(e) => {
                    record(state, id, ti, pi, iteration, warmup, "error", Some(&e.to_string()), &Metrics::default(), "")?;
                    failures_in_a_row += 1;
                }
            }
            notify();
            if failures_in_a_row >= GIVE_UP_AFTER {
                break;
            }
        }
        // Memory in use while the model is loaded, where the runtime reports it.
        if let Ok(p) = &provider {
            if let Ok(loaded) = runtime::loaded(state, p).await {
                if let Some(m) = loaded.iter().find(|m| m.name == target.model) {
                    state.db.conn().execute(
                        "UPDATE lab_bench_runs SET memory_bytes = ?3, memory_vram_bytes = ?4 WHERE suite_id = ?1 AND target = ?2 AND status = 'ok'",
                        params![id, ti as i64, m.size_bytes.map(|v| v as i64), m.size_vram_bytes.map(|v| v as i64)],
                    )?;
                    notify();
                }
            }
        }
    }
    let (status, error) = if stopped {
        ("cancelled", None)
    } else if any_ok {
        ("completed", None)
    } else {
        ("failed", Some("No request succeeded. Open the runs to see each error."))
    };
    state.db.conn().execute(
        "UPDATE lab_bench_suites SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
        params![id, status, error, db::now()],
    )?;
    Ok(())
}

// ------------------------------------------------------------------ export

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// One row per run. An unknown value is an empty cell, never a zero.
pub fn to_csv(suite: &Suite) -> String {
    let num = |v: Option<f64>| v.map(|x| format!("{x:.3}")).unwrap_or_default();
    let int = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_default();
    let mut out = String::from(
        "benchmark,started,os,cpu,memory_bytes,provider,runtime,runtime_version,endpoint,model,quantization,parameters,prompt,iteration,warmup,status,\
         ttft_ms,total_ms,tokens_per_second,tokens_per_second_source,input_tokens,output_tokens,load_ms,prompt_eval_ms,eval_ms,model_memory_bytes,model_vram_bytes,\
         temperature,top_p,max_tokens,seed,error\n",
    );
    let hw = &suite.hardware;
    let p = &suite.config.params;
    for r in &suite.runs {
        let Some(t) = suite.config.targets.get(r.target) else { continue };
        let prompt = suite.config.prompts.get(r.prompt).map(|p| p.label.as_str()).unwrap_or("");
        let m = &r.metrics;
        let cells: Vec<String> = vec![
            suite.name.clone(),
            suite.created_at.clone(),
            format!("{} {}", hw.os, hw.os_version.clone().unwrap_or_default()).trim().to_string(),
            hw.cpu.clone().unwrap_or_default(),
            int(hw.memory_bytes),
            t.provider_name.clone(),
            t.runtime.clone(),
            t.runtime_version.clone().unwrap_or_default(),
            t.endpoint.clone(),
            t.model.clone(),
            t.quantization.clone().unwrap_or_default(),
            t.parameters.clone().unwrap_or_default(),
            prompt.to_string(),
            r.iteration.to_string(),
            r.warmup.to_string(),
            r.status.clone(),
            num(m.ttft_ms),
            if r.status == "ok" { num(Some(m.total_ms)) } else { String::new() },
            num(m.tokens_per_second),
            m.tps_source.clone().unwrap_or_default(),
            int(m.input_tokens),
            int(m.output_tokens),
            num(m.load_ms),
            num(m.prompt_eval_ms),
            num(m.eval_ms),
            int(r.memory_bytes),
            int(r.memory_vram_bytes),
            num(p.temperature),
            num(p.top_p),
            int(p.max_tokens.map(u64::from)),
            p.seed.map(|s| s.to_string()).unwrap_or_default(),
            r.error.clone().unwrap_or_default(),
        ];
        out.push_str(&cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

pub fn to_json(suite: &Suite) -> Result<String> {
    Ok(serde_json::to_string_pretty(suite)?)
}

// ------------------------------------------------------------------ commands

/// Start a benchmark. Returns its id once it is accepted; progress arrives as
/// `nexus://lab-bench` events carrying that id.
#[tauri::command]
pub async fn lab_bench_start<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, input: BenchInput) -> Result<String> {
    if state.lab.bench.count() > 0 {
        return Err("A benchmark is already running. Models are tested one at a time so results stay comparable.".into());
    }
    let (name, config) = prepare(&state, &input).await?;
    let id = create(&state, &name, &config)?;
    let cancel = state.lab.bench.register(&id)?;
    let (task_app, task_id, allow_remote) = (app.clone(), id.clone(), input.allow_remote);
    tauri::async_runtime::spawn(async move {
        let state = task_app.state::<AppState>();
        let (emitter, event_id) = (task_app.clone(), task_id.clone());
        let notify = move || {
            let _ = emitter.emit(EVENT_BENCH, &event_id);
        };
        if let Err(e) = run_suite(&state, &task_id, cancel, allow_remote, &notify).await {
            let _ = state.db.conn().execute(
                "UPDATE lab_bench_suites SET status = 'failed', error = ?2, finished_at = ?3 WHERE id = ?1",
                params![task_id, e.to_string(), db::now()],
            );
        }
        state.lab.bench.finish(&task_id);
        let _ = task_app.emit(EVENT_BENCH, &task_id);
    });
    let _ = app.emit(EVENT_BENCH, &id);
    Ok(id)
}

#[tauri::command]
pub fn lab_bench_cancel(state: State<'_, AppState>, id: String) -> bool {
    state.lab.bench.cancel(&id)
}

#[tauri::command]
pub fn lab_bench_list(state: State<'_, AppState>) -> Result<Vec<SuiteSummary>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.status, s.created_at, s.config, (SELECT COUNT(*) FROM lab_bench_runs r WHERE r.suite_id = s.id)
         FROM lab_bench_suites s ORDER BY s.created_at DESC, s.rowid DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let config: SuiteConfig = serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default();
        Ok(SuiteSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            status: r.get(2)?,
            created_at: r.get(3)?,
            models: config.targets.iter().map(|t| t.model.clone()).collect(),
            prompts: config.prompts.len(),
            repeats: config.repeats,
            runs: r.get::<_, i64>(5)? as usize,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn lab_bench_get(state: State<'_, AppState>, id: String) -> Result<Suite> {
    load_suite(&state, &id)
}

#[tauri::command]
pub fn lab_bench_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    if state.lab.bench.is_running(&id) {
        return Err("Cancel the benchmark before deleting it".into());
    }
    state.db.conn().execute("DELETE FROM lab_bench_suites WHERE id = ?1", [id])?;
    Ok(())
}

/// Save a benchmark as JSON or CSV to a location picked in a native dialog opened by the core.
#[tauri::command]
pub async fn lab_bench_export<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    format: String,
) -> Result<Option<String>> {
    let suite = load_suite(&state, &id)?;
    let (content, extension, filter) = match format.as_str() {
        "json" => (to_json(&suite)?, "json", "JSON"),
        "csv" => (to_csv(&suite), "csv", "CSV"),
        _ => return Err("Export as json or csv".into()),
    };
    let stem: String =
        suite.name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' }).take(60).collect();
    let file_name = format!("nexus-benchmark-{}.{extension}", stem.trim_matches('-'));
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_file_name(file_name).add_filter(filter, &[extension]).blocking_save_file()
    })
    .await
    .map_err(|e| AppError::Msg(format!("Could not open the save dialog: {e}")))?;
    let Some(file) = picked else { return Ok(None) };
    let path = file.into_path().map_err(|e| AppError::Msg(format!("Unsupported save location: {e}")))?;
    std::fs::write(&path, content)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// The same export, returned as text for the caller to show or copy.
#[tauri::command]
pub fn lab_bench_export_text(state: State<'_, AppState>, id: String, format: String) -> Result<String> {
    let suite = load_suite(&state, &id)?;
    match format.as_str() {
        "json" => to_json(&suite),
        "csv" => Ok(to_csv(&suite)),
        _ => Err("Export as json or csv".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testserver::{serve, Resp};
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn add_provider(state: &AppState, id: &str, kind: &str, url: &str) {
        state
            .db
            .conn()
            .execute(
                "INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES (?1, ?1, ?2, ?3, 0, ?1)",
                params![id, kind, url],
            )
            .unwrap();
    }

    fn input(targets: &[(&str, &str)], repeats: u32) -> BenchInput {
        BenchInput {
            name: Some("test".into()),
            targets: targets.iter().map(|(p, m)| Target { provider_id: p.to_string(), model: m.to_string() }).collect(),
            prompts: vec![
                Prompt { label: String::new(), text: "Count to three".into() },
                Prompt { label: "Second".into(), text: "Say hi".into() },
            ],
            system: None,
            params: GenParams { temperature: Some(0.0), max_tokens: Some(32), seed: Some(1), ..Default::default() },
            repeats,
            warmup: true,
            cold_start: false,
            allow_remote: false,
        }
    }

    /// An Ollama-like server whose first request per model "loads" it, and which reports its
    /// own token counts and timings.
    fn ollama_like(calls: Arc<AtomicUsize>) -> impl Fn(&crate::lab::testserver::Req) -> Resp + Send + Sync + 'static {
        move |req| match req.path.as_str() {
            "/api/chat" => {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                let model = req.json()["model"].as_str().unwrap_or("").to_string();
                if model == "broken:1" {
                    return Resp::error(500, json!({"error": "model failed to load"}));
                }
                let load = if n == 0 { 2_000_000_000u64 } else { 5_000_000 };
                Resp::stream("application/x-ndjson", vec![
                    (30, "{\"message\":{\"content\":\"one two\"},\"done\":false}\n".into()),
                    (20, format!("{{\"message\":{{\"content\":\" three\"}},\"done\":true,\"done_reason\":\"stop\",\"load_duration\":{load},\"prompt_eval_count\":7,\"prompt_eval_duration\":40000000,\"eval_count\":10,\"eval_duration\":{}}}\n", 200_000_000 + (n as u64 % 3) * 50_000_000)),
                ])
            }
            "/api/ps" => Resp::json(json!({"models": [{"name": "fast:1", "size": 4000, "size_vram": 3000, "context_length": 4096}]})),
            "/api/version" => Resp::json(json!({"version": "0.40.16"})),
            "/api/tags" => Resp::json(json!({"models": []})),
            "/api/generate" => Resp::json(json!({"done": true})),
            _ => Resp::error(404, json!({"error": "nope"})),
        }
    }

    async fn run(state: &AppState, input: &BenchInput) -> Suite {
        let (name, config) = prepare(state, input).await.unwrap();
        let id = create(state, &name, &config).unwrap();
        let cancel = state.lab.bench.register(&id).unwrap();
        run_suite(state, &id, cancel, input.allow_remote, &|| {}).await.unwrap();
        state.lab.bench.finish(&id);
        load_suite(state, &id).unwrap()
    }

    #[test]
    fn statistics_are_computed_from_the_measured_values_only() {
        assert_eq!(stat(&[]), None);
        let one = stat(&[5.0]).unwrap();
        assert_eq!((one.n, one.mean, one.median, one.stddev), (1, 5.0, 5.0, 0.0));
        let s = stat(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!((s.n, s.mean, s.median, s.min, s.max), (8, 5.0, 4.5, 2.0, 9.0));
        assert!((s.stddev - 2.138089935).abs() < 1e-6, "sample standard deviation, got {}", s.stddev);
        assert_eq!(stat(&[3.0, 1.0, 2.0]).unwrap().median, 2.0);
        assert_eq!(stat(&[f64::NAN, 1.0]).unwrap().n, 1, "non-finite values are not measurements");
    }

    #[tokio::test]
    async fn a_benchmark_runs_every_prompt_for_every_model_and_summarises_real_measurements() {
        let calls = Arc::new(AtomicUsize::new(0));
        let server = serve(ollama_like(calls.clone())).await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        // What the library knew about the model is stored with the results.
        state
            .db
            .conn()
            .execute(
                "INSERT INTO lab_models (key, provider_id, model_id, meta, first_seen, last_seen) VALUES (?1, 'o', 'fast:1', ?2, '', '')",
                params![
                    registry::provider_key("o", "fast:1"),
                    json!({"quantization": "Q4_K_M", "parameter_count": 7_600_000_000u64, "context_length": 8192, "format": "gguf"})
                        .to_string()
                ],
            )
            .unwrap();

        let suite = run(&state, &input(&[("o", "fast:1")], 3)).await;
        assert_eq!(suite.status, "completed");
        assert_eq!((suite.planned_runs, suite.runs.len()), (7, 7), "one warm-up plus two prompts three times each");
        assert_eq!(calls.load(Ordering::SeqCst), 7, "requests are sent one at a time, exactly as planned");
        assert!(suite.runs[0].warmup && suite.runs.iter().skip(1).all(|r| !r.warmup));
        let order: Vec<(usize, u32)> = suite.runs.iter().map(|r| (r.prompt, r.iteration)).collect();
        assert_eq!(order, [(0, 0), (0, 1), (0, 2), (0, 3), (1, 1), (1, 2), (1, 3)]);

        let t = &suite.config.targets[0];
        assert_eq!(
            (t.runtime.as_str(), t.runtime_version.as_deref(), t.quantization.as_deref(), t.parameters.as_deref(), t.context_length),
            ("ollama", Some("0.40.16"), Some("Q4_K_M"), Some("7.6B"), Some(8192))
        );
        assert!(t.local && t.endpoint.starts_with("127.0.0.1:"));
        assert_eq!(suite.config.prompts[0].label, "Prompt 1", "unnamed prompts get a label");
        assert_eq!(suite.hardware.os, std::env::consts::OS);

        let s = &suite.summary[0];
        assert_eq!((s.runs, s.errors), (6, 0), "the warm-up is not part of the statistics");
        assert_eq!(s.load_ms, Some(2000.0), "load time comes from the request that loaded the model");
        let ttft = s.ttft_ms.as_ref().unwrap();
        assert!(ttft.n == 6 && ttft.min >= 25.0 && ttft.max < 3000.0, "{ttft:?}");
        assert!(s.total_ms.as_ref().unwrap().min >= ttft.min);
        let tps = s.tokens_per_second.as_ref().unwrap();
        assert_eq!(s.tps_source.as_deref(), Some("runtime"));
        assert!(
            (tps.max - 50.0).abs() < 1e-9 && (tps.min - 10.0 / 0.3).abs() < 1e-9,
            "speeds follow the runtime's own generation times: {tps:?}"
        );
        assert_eq!(s.output_tokens.as_ref().unwrap().mean, 10.0);
        assert_eq!((s.memory_bytes, s.memory_vram_bytes), (Some(4000), Some(3000)));
        assert_eq!(suite.runs[1].output, "one two three");

        let sent = server.last("/api/chat").json();
        assert_eq!(
            (sent["options"]["temperature"].as_f64(), sent["options"]["num_predict"].as_u64(), sent["options"]["seed"].as_i64()),
            (Some(0.0), Some(32), Some(1))
        );
        assert_eq!(sent["messages"][0]["content"], "Say hi");
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn failing_models_are_recorded_and_skipped_without_stopping_the_rest() {
        let server = serve(ollama_like(Arc::new(AtomicUsize::new(1)))).await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        add_provider(&state, "dead", "ollama", "http://127.0.0.1:9");
        let suite = run(&state, &input(&[("o", "broken:1"), ("dead", "x:1"), ("o", "fast:1")], 2)).await;
        assert_eq!(suite.status, "completed");
        let of = |t: usize| suite.runs.iter().filter(|r| r.target == t).collect::<Vec<_>>();
        assert_eq!(of(0).len(), 2, "after two failures in a row the model is skipped");
        assert!(of(0).iter().all(|r| r.status == "error" && r.error.as_deref().unwrap().contains("model failed to load")));
        assert!(of(1)[0].error.as_deref().unwrap().contains("Could not connect"));
        assert_eq!(of(2).iter().filter(|r| r.status == "ok").count(), 5);
        assert_eq!((suite.summary[0].runs, suite.summary[0].errors), (0, 2));
        assert!(
            suite.summary[0].ttft_ms.is_none() && suite.summary[0].tokens_per_second.is_none(),
            "a model with no successful run has no figures"
        );

        // When nothing works at all, the suite says so.
        let failed = run(&state, &input(&[("dead", "x:1")], 1)).await;
        assert_eq!(failed.status, "failed");
        assert!(failed.error.unwrap().contains("No request succeeded"));
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn a_benchmark_can_be_cancelled_mid_request() {
        let server = serve(|req| match req.path.as_str() {
            "/api/chat" => {
                Resp::stream_then_hang("application/x-ndjson", vec![(0, "{\"message\":{\"content\":\"working\"},\"done\":false}\n".into())])
            }
            "/api/version" => Resp::json(json!({"version": "1"})),
            _ => Resp::json(json!({"models": []})),
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        let spec = input(&[("o", "slow:1")], 5);
        let (name, config) = prepare(&state, &spec).await.unwrap();
        let id = create(&state, &name, &config).unwrap();
        let cancel = state.lab.bench.register(&id).unwrap();
        let (result, _) = tokio::join!(run_suite(&state, &id, cancel, false, &|| {}), async {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            assert!(state.lab.bench.cancel(&id));
        });
        result.unwrap();
        let suite = load_suite(&state, &id).unwrap();
        assert_eq!(suite.status, "cancelled");
        assert_eq!(suite.runs.len(), 1, "no further requests were sent after the cancel");
        assert_eq!((suite.runs[0].status.as_str(), suite.runs[0].output.as_str()), ("cancelled", "working"));
        assert_eq!(suite.summary[0].runs, 0, "a cancelled run is not a measurement");
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn benchmark_requests_are_validated_and_remote_targets_need_confirmation() {
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", "http://127.0.0.1:9");
        add_provider(&state, "remote", "openai", "https://api.example.invalid/v1");
        add_provider(&state, "off", "ollama", "http://127.0.0.1:9");
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'off'", []).unwrap();
        let ok = input(&[("o", "a:1")], 2);
        assert!(prepare(&state, &ok).await.is_ok());
        let err = |i: BenchInput| {
            let state = &state;
            async move { prepare(state, &i).await.unwrap_err().to_string() }
        };
        assert!(err(BenchInput { targets: vec![], ..ok.clone() }).await.contains("Choose between"));
        assert!(err(BenchInput { prompts: vec![], ..ok.clone() }).await.contains("prompts"));
        assert!(err(BenchInput { repeats: 0, ..ok.clone() }).await.contains("Repeat"));
        assert!(err(BenchInput { repeats: 99, ..ok.clone() }).await.contains("Repeat"));
        assert!(err(BenchInput { prompts: vec![Prompt { label: "x".into(), text: "  ".into() }], ..ok.clone() }).await.contains("empty"));
        assert!(err(BenchInput { params: GenParams { top_p: Some(4.0), ..Default::default() }, ..ok.clone() }).await.contains("Top-p"));
        assert!(err(input(&[("o", "a:1"), ("o", "a:1")], 1)).await.contains("twice"));
        assert!(err(input(&[("missing", "a:1")], 1)).await.contains("not found"));
        assert!(err(input(&[("off", "a:1")], 1)).await.contains("disabled"));
        assert!(err(input(&[("o", "  ")], 1)).await.contains("needs a model"));
        let remote = err(input(&[("remote", "gpt")], 1)).await;
        assert!(remote.contains("not on this machine") && remote.contains("api.example.invalid"), "{remote}");
        let confirmed = BenchInput { allow_remote: true, ..input(&[("remote", "gpt")], 1) };
        let (_, config) = prepare(&state, &confirmed).await.unwrap();
        assert!(!config.targets[0].local);
        let big = BenchInput {
            repeats: 20,
            prompts: (0..10).map(|i| Prompt { label: format!("p{i}"), text: "x".into() }).collect(),
            ..input(&[("o", "a"), ("o", "b"), ("o", "c")], 20)
        };
        assert!(err(big).await.contains("400 or fewer"));
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn exports_keep_unknown_values_empty_and_escape_text() {
        let config = SuiteConfig {
            targets: vec![TargetInfo {
                provider_id: "o".into(),
                provider_name: "Local, \"main\"".into(),
                runtime: "ollama".into(),
                runtime_version: Some("0.40.16".into()),
                endpoint: "127.0.0.1:11434".into(),
                local: true,
                model: "m:1".into(),
                quantization: None,
                parameters: Some("7B".into()),
                context_length: None,
                format: None,
            }],
            prompts: vec![Prompt { label: "Short".into(), text: "hi".into() }],
            system: None,
            params: GenParams { temperature: Some(0.0), max_tokens: Some(64), ..Default::default() },
            repeats: 1,
            warmup: false,
            cold_start: false,
        };
        let ok = Metrics {
            ttft_ms: Some(120.5),
            total_ms: 900.25,
            input_tokens: Some(5),
            output_tokens: None,
            tokens_per_second: None,
            ..Default::default()
        };
        let runs = vec![
            Run {
                id: 1,
                target: 0,
                prompt: 0,
                iteration: 1,
                warmup: false,
                status: "ok".into(),
                error: None,
                metrics: ok,
                memory_bytes: None,
                memory_vram_bytes: None,
                output: "x".into(),
                at: "t".into(),
            },
            Run {
                id: 2,
                target: 0,
                prompt: 0,
                iteration: 2,
                warmup: false,
                status: "error".into(),
                error: Some("line one\nline \"two\"".into()),
                metrics: Metrics::default(),
                memory_bytes: None,
                memory_vram_bytes: None,
                output: String::new(),
                at: "t".into(),
            },
        ];
        let suite = Suite {
            id: "s".into(),
            name: "Demo".into(),
            hardware: Hardware {
                os: "macos".into(),
                os_version: Some("27.2".into()),
                arch: "aarch64".into(),
                cpu: Some("Apple M5 Pro".into()),
                cores: Some(12),
                memory_bytes: Some(25769803776),
            },
            status: "completed".into(),
            error: None,
            created_at: "2026-11-15T10:00:00Z".into(),
            finished_at: None,
            planned_runs: planned_runs(&config),
            summary: summarize(&config, &runs),
            config,
            runs,
        };
        let csv = to_csv(&suite);
        let rows: Vec<&str> = csv.split("\nDemo,").collect();
        assert_eq!(rows.len(), 3, "a header and two runs: {csv}");
        assert_eq!(csv.lines().next().unwrap().split(',').count(), 32);
        assert!(rows[1].contains("macos 27.2,Apple M5 Pro,25769803776,\"Local, \"\"main\"\"\",ollama,0.40.16,127.0.0.1:11434,m:1,,7B,Short,1,false,ok,120.500,900.250,,,5,,,,,,,0.000,,64,,"), "{}", rows[1]);
        assert!(
            rows[2].contains(",2,false,error,,,,,,,,,,,,0.000,,64,,\"line one\nline \"\"two\"\"\""),
            "a failed run has no timings, not zeros: {}",
            rows[2]
        );

        let json: serde_json::Value = serde_json::from_str(&to_json(&suite).unwrap()).unwrap();
        assert_eq!(json["hardware"]["cpu"], "Apple M5 Pro");
        assert!(json["runs"][0]["metrics"]["output_tokens"].is_null());
        assert_eq!(json["summary"][0]["runs"], 1);
        assert!(json["summary"][0]["tokens_per_second"].is_null());
    }
}
