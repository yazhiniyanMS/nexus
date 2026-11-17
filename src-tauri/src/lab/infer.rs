//! Streaming inference against a connection, with timings measured here and token counts taken
//! from the runtime. A number the runtime did not report stays empty; nothing is estimated
//! except tokens per second, which is labelled "derived" when NEXUS had to compute it.
//!
//! Three wire formats are spoken: Ollama's `/api/chat` (JSON lines), the OpenAI chat API
//! (server-sent events; also used for llama.cpp, whose `timings` are read when present) and
//! the Anthropic Messages API (server-sent events).
//!
//! Model output is only ever handled as text. Playground sessions are stored locally.

use super::{cancelled, GenParams, Lines};
use crate::ai::{self, ChatMessage, Provider};
use crate::error::{AppError, Result};
use crate::{db, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::watch;

pub const EVENT_INFER: &str = "nexus://lab-infer";

/// How long a stream may stay silent before the request is given up.
const STALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Output cap sent to the Anthropic API (which requires one) when the user set none.
const ANTHROPIC_STREAM_MAX_TOKENS: u32 = 64_000;
const MAX_PROMPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SESSION_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InferRequest {
    /// Chosen by the caller, so its events can be matched from the first one on.
    pub id: String,
    pub provider_id: String,
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub params: GenParams,
    /// Must be true to send the prompt to an endpoint that is not on this machine.
    #[serde(default)]
    pub allow_remote: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Metrics {
    /// Request sent to first generated text (thinking included). Measured by NEXUS.
    pub ttft_ms: Option<f64>,
    /// Request sent to end of stream. Measured by NEXUS.
    pub total_ms: f64,
    /// Reported by the runtime.
    pub input_tokens: Option<u64>,
    /// Reported by the runtime.
    pub output_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
    /// "runtime" when the runtime timed its own generation, "derived" when NEXUS divided the
    /// reported output tokens by the time after the first token.
    pub tps_source: Option<String>,
    /// Time the runtime spent loading the model for this request, when it says.
    pub load_ms: Option<f64>,
    /// Time the runtime spent processing the prompt, when it says.
    pub prompt_eval_ms: Option<f64>,
    /// Time the runtime spent generating, when it says.
    pub eval_ms: Option<f64>,
    pub stop_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outcome {
    pub text: String,
    pub thinking: String,
    pub metrics: Metrics,
    pub model: String,
    pub provider_name: String,
    /// The connection kind that served the request.
    pub runtime: String,
    pub local: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(String),
    Thinking(String),
}

// ------------------------------------------------------------------ request bodies

fn with_system(system: Option<&str>, messages: &[ChatMessage]) -> Vec<Value> {
    let mut out = vec![];
    if let Some(s) = system.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(json!({ "role": "system", "content": s }));
    }
    out.extend(messages.iter().map(|m| json!({ "role": m.role, "content": m.content })));
    out
}

/// The request body for a kind of runtime. Only settings that kind accepts are included, and
/// only when they were set.
pub fn build_body(kind: &str, model: &str, system: Option<&str>, messages: &[ChatMessage], params: &GenParams) -> Value {
    let p = params.for_kind(kind);
    let set = |target: &mut Value, key: &str, value: Option<Value>| {
        if let Some(v) = value {
            target[key] = v;
        }
    };
    match kind {
        "ollama" => {
            let mut options = json!({});
            set(&mut options, "temperature", p.temperature.map(Value::from));
            set(&mut options, "top_p", p.top_p.map(Value::from));
            set(&mut options, "top_k", p.top_k.map(Value::from));
            set(&mut options, "num_predict", p.max_tokens.map(Value::from));
            set(&mut options, "seed", p.seed.map(Value::from));
            set(&mut options, "repeat_penalty", p.repeat_penalty.map(Value::from));
            let mut body = json!({ "model": model, "messages": with_system(system, messages), "stream": true });
            if options.as_object().is_some_and(|o| !o.is_empty()) {
                body["options"] = options;
            }
            set(&mut body, "think", p.think.map(Value::from));
            body
        }
        "anthropic" => {
            // No sampling settings: current Claude models reject them.
            let mut body = json!({
                "model": model,
                "max_tokens": p.max_tokens.unwrap_or(ANTHROPIC_STREAM_MAX_TOKENS),
                "messages": messages.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect::<Vec<_>>(),
                "stream": true,
            });
            set(&mut body, "system", system.map(str::trim).filter(|s| !s.is_empty()).map(Value::from));
            body
        }
        _ => {
            let mut body = json!({
                "model": model,
                "messages": with_system(system, messages),
                "stream": true,
                "stream_options": { "include_usage": true },
            });
            set(&mut body, "temperature", p.temperature.map(Value::from));
            set(&mut body, "top_p", p.top_p.map(Value::from));
            set(&mut body, "max_tokens", p.max_tokens.map(Value::from));
            set(&mut body, "seed", p.seed.map(Value::from));
            set(&mut body, "top_k", p.top_k.map(Value::from));
            set(&mut body, "repeat_penalty", p.repeat_penalty.map(Value::from));
            body
        }
    }
}

// ------------------------------------------------------------------ stream parsing

/// Everything a stream has said so far.
#[derive(Default, Debug)]
pub struct StreamState {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Ollama: nanoseconds.
    pub load_ns: Option<u64>,
    pub prompt_eval_ns: Option<u64>,
    pub eval_ns: Option<u64>,
    /// llama.cpp-style `timings`: milliseconds and tokens per second.
    pub prompt_ms: Option<f64>,
    pub predicted_ms: Option<f64>,
    pub predicted_per_second: Option<f64>,
    pub stop_reason: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}

fn error_text(v: &Value) -> Option<String> {
    let e = v.get("error")?;
    e.get("message").and_then(|m| m.as_str()).or(e.as_str()).map(String::from).or_else(|| Some(e.to_string()))
}

fn push(out: &mut Vec<Piece>, piece: Option<&str>, thinking: bool) {
    if let Some(text) = piece.filter(|t| !t.is_empty()) {
        out.push(if thinking { Piece::Thinking(text.to_string()) } else { Piece::Text(text.to_string()) });
    }
}

/// The payload of a server-sent-events `data:` line; None for comments, event names and blanks.
fn sse_data(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(str::trim)
}

impl StreamState {
    /// One line of Ollama's `/api/chat` stream.
    pub fn ollama_line(&mut self, line: &str) -> Vec<Piece> {
        let mut out = vec![];
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return out };
        if let Some(e) = error_text(&v) {
            self.error = Some(e);
            return out;
        }
        push(&mut out, v.pointer("/message/thinking").and_then(|t| t.as_str()), true);
        push(&mut out, v.pointer("/message/content").and_then(|t| t.as_str()), false);
        if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
            self.done = true;
            self.stop_reason = v.get("done_reason").and_then(|r| r.as_str()).map(String::from);
            self.input_tokens = v.get("prompt_eval_count").and_then(|n| n.as_u64());
            self.output_tokens = v.get("eval_count").and_then(|n| n.as_u64());
            self.load_ns = v.get("load_duration").and_then(|n| n.as_u64());
            self.prompt_eval_ns = v.get("prompt_eval_duration").and_then(|n| n.as_u64());
            self.eval_ns = v.get("eval_duration").and_then(|n| n.as_u64());
        }
        out
    }

    /// One line of an OpenAI-style chat completion stream.
    pub fn openai_line(&mut self, line: &str) -> Vec<Piece> {
        let mut out = vec![];
        let Some(data) = sse_data(line) else {
            // Some servers answer a bad request with a bare JSON error instead of an event.
            if let Some(e) = serde_json::from_str::<Value>(line.trim()).ok().as_ref().and_then(error_text) {
                self.error = Some(e);
            }
            return out;
        };
        if data == "[DONE]" {
            self.done = true;
            return out;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { return out };
        if let Some(e) = error_text(&v) {
            self.error = Some(e);
            return out;
        }
        if let Some(choice) = v.pointer("/choices/0") {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            push(&mut out, delta.get("reasoning_content").or(delta.get("reasoning")).and_then(|t| t.as_str()), true);
            push(&mut out, delta.get("content").and_then(|t| t.as_str()), false);
            if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                self.stop_reason = Some(reason.to_string());
            }
        }
        if let Some(usage) = v.get("usage").filter(|u| u.is_object()) {
            self.input_tokens = usage.get("prompt_tokens").and_then(|n| n.as_u64()).or(self.input_tokens);
            self.output_tokens = usage.get("completion_tokens").and_then(|n| n.as_u64()).or(self.output_tokens);
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            self.prompt_ms = t.get("prompt_ms").and_then(|n| n.as_f64());
            self.predicted_ms = t.get("predicted_ms").and_then(|n| n.as_f64());
            self.predicted_per_second = t.get("predicted_per_second").and_then(|n| n.as_f64());
            self.input_tokens = self.input_tokens.or(t.get("prompt_n").and_then(|n| n.as_u64()));
            self.output_tokens = self.output_tokens.or(t.get("predicted_n").and_then(|n| n.as_u64()));
        }
        out
    }

    /// One line of an Anthropic Messages stream.
    pub fn anthropic_line(&mut self, line: &str) -> Vec<Piece> {
        let mut out = vec![];
        let Some(data) = sse_data(line) else { return out };
        let Ok(v) = serde_json::from_str::<Value>(data) else { return out };
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message_start" => {
                let usage = v.pointer("/message/usage").unwrap_or(&Value::Null);
                let count = |key: &str| usage.get(key).and_then(|n| n.as_u64());
                // Cached prompt tokens are still prompt tokens.
                if let Some(input) = count("input_tokens") {
                    self.input_tokens =
                        Some(input + count("cache_read_input_tokens").unwrap_or(0) + count("cache_creation_input_tokens").unwrap_or(0));
                }
            }
            "content_block_delta" => match v.pointer("/delta/type").and_then(|t| t.as_str()) {
                Some("text_delta") => push(&mut out, v.pointer("/delta/text").and_then(|t| t.as_str()), false),
                Some("thinking_delta") => push(&mut out, v.pointer("/delta/thinking").and_then(|t| t.as_str()), true),
                _ => {}
            },
            "message_delta" => {
                if let Some(reason) = v.pointer("/delta/stop_reason").and_then(|r| r.as_str()) {
                    self.stop_reason = Some(reason.to_string());
                }
                self.output_tokens = v.pointer("/usage/output_tokens").and_then(|n| n.as_u64()).or(self.output_tokens);
            }
            "message_stop" => self.done = true,
            "error" => self.error = error_text(&v).or(Some("The provider reported an error".into())),
            _ => {}
        }
        out
    }
}

/// Turn what the stream reported, plus the two times measured here, into metrics.
pub fn metrics(kind: &str, s: &StreamState, ttft_ms: Option<f64>, total_ms: f64) -> Metrics {
    let ms = |ns: Option<u64>| ns.map(|n| n as f64 / 1e6);
    let mut m = Metrics {
        ttft_ms,
        total_ms,
        input_tokens: s.input_tokens,
        output_tokens: s.output_tokens,
        stop_reason: s.stop_reason.clone(),
        ..Default::default()
    };
    if kind == "ollama" {
        m.load_ms = ms(s.load_ns);
        m.prompt_eval_ms = ms(s.prompt_eval_ns);
        m.eval_ms = ms(s.eval_ns);
        if let (Some(tokens), Some(ns)) = (s.output_tokens, s.eval_ns.filter(|n| *n > 0)) {
            m.tokens_per_second = Some(tokens as f64 / (ns as f64 / 1e9));
            m.tps_source = Some("runtime".into());
        }
    } else {
        m.prompt_eval_ms = s.prompt_ms;
        m.eval_ms = s.predicted_ms;
        if let Some(tps) = s.predicted_per_second.filter(|t| t.is_finite() && *t > 0.0) {
            m.tokens_per_second = Some(tps);
            m.tps_source = Some("runtime".into());
        }
    }
    if m.tokens_per_second.is_none() {
        // Fall back to the client's clock, only when the runtime reported how many tokens it made.
        if let (Some(tokens), Some(first)) = (s.output_tokens.filter(|t| *t > 1), ttft_ms) {
            let generating = (total_ms - first) / 1000.0;
            if generating > 0.0 {
                m.tokens_per_second = Some(tokens as f64 / generating);
                m.tps_source = Some("derived".into());
            }
        }
    }
    m
}

// ------------------------------------------------------------------ running

pub fn validate(req: &InferRequest) -> Result<()> {
    let id_ok = (8..=64).contains(&req.id.len()) && req.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !id_ok {
        return Err("Invalid request id".into());
    }
    let model = req.model.trim();
    if model.is_empty() || model.chars().count() > 200 {
        return Err("Choose a model".into());
    }
    if req.messages.is_empty() || req.messages.len() > 400 {
        return Err("Enter a prompt".into());
    }
    if req.messages.iter().any(|m| !matches!(m.role.as_str(), "user" | "assistant")) {
        return Err("Messages must come from the user or the assistant".into());
    }
    if req.messages.last().is_some_and(|m| m.role != "user" || m.content.trim().is_empty()) {
        return Err("Enter a prompt".into());
    }
    let size: usize = req.messages.iter().map(|m| m.content.len()).sum::<usize>() + req.system.as_ref().map(|s| s.len()).unwrap_or(0);
    if size > MAX_PROMPT_BYTES {
        return Err("The prompt is too large to send".into());
    }
    req.params.validate()
}

/// The provider a request may use: enabled, and local unless sending elsewhere was confirmed.
pub fn provider_for(state: &AppState, req: &InferRequest) -> Result<Provider> {
    let p = ai::usable_provider(state, &req.provider_id)?;
    if !p.local && !req.allow_remote {
        let host = reqwest::Url::parse(&p.base_url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| p.base_url.clone());
        return Err(AppError::Msg(format!("“{}” is not on this machine: the prompt would be sent to {host}. Confirm that first.", p.name)));
    }
    Ok(p)
}

fn outcome(p: &Provider, model: &str, text: String, thinking: String, metrics: Metrics, cancelled: bool) -> Outcome {
    Outcome {
        text,
        thinking,
        metrics,
        model: model.to_string(),
        provider_name: p.name.clone(),
        runtime: p.kind.clone(),
        local: p.local,
        cancelled,
    }
}

/// Run one streamed request. `on_piece` receives text as it arrives. A cancelled request ends
/// normally, with what had been generated so far.
pub async fn run(
    state: &AppState,
    req: &InferRequest,
    cancel: &mut watch::Receiver<bool>,
    on_piece: &mut (dyn FnMut(&Piece) + Send),
) -> Result<Outcome> {
    validate(req)?;
    let p = provider_for(state, req)?;
    let model = req.model.trim();
    let body = build_body(&p.kind, model, req.system.as_deref(), &req.messages, &req.params);
    let url = match p.kind.as_str() {
        "ollama" => format!("{}/api/chat", p.base_url),
        "anthropic" => format!("{}/v1/messages", p.base_url),
        _ => format!("{}/chat/completions", ai::openai_base(&p)),
    };
    let ai::Authed { req: request, redactor } = ai::authed(&p, state.http.post(url).json(&body))?;

    let started = Instant::now();
    let elapsed = |since: Instant| since.elapsed().as_secs_f64() * 1000.0;
    let mut text = String::new();
    let mut thinking = String::new();
    let stopped = |text: String, thinking: String| {
        let m = Metrics { total_ms: elapsed(started), stop_reason: Some("cancelled".into()), ..Default::default() };
        Ok(outcome(&p, model, text, thinking, m, true))
    };
    if cancelled(cancel) {
        return stopped(text, thinking);
    }
    // A runtime may load the model before it answers at all, so the wait itself is cancellable.
    let mut resp = tokio::select! {
        resp = request.send() => resp.map_err(|e| ai::transport_error(e, &redactor))?,
        _ = cancel.changed() => return stopped(text, thinking),
    };
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ai::error_detail(status, &body, &redactor));
    }

    let mut lines = Lines::default();
    let mut stream = StreamState::default();
    let mut ttft_ms = None;
    let mut handle = |stream: &mut StreamState, line: &str, text: &mut String, thinking: &mut String, ttft_ms: &mut Option<f64>| {
        let pieces = match p.kind.as_str() {
            "ollama" => stream.ollama_line(line),
            "anthropic" => stream.anthropic_line(line),
            _ => stream.openai_line(line),
        };
        for piece in pieces {
            if ttft_ms.is_none() {
                *ttft_ms = Some(elapsed(started));
            }
            match &piece {
                Piece::Text(t) => text.push_str(t),
                Piece::Thinking(t) => thinking.push_str(t),
            }
            on_piece(&piece);
        }
    };
    loop {
        let chunk = tokio::select! {
            chunk = tokio::time::timeout(STALL_TIMEOUT, resp.chunk()) => chunk,
            // Dropping the response closes the connection, which is what stops the runtime.
            _ = cancel.changed() => {
                let mut m = metrics(&p.kind, &stream, ttft_ms, elapsed(started));
                m.stop_reason = Some("cancelled".into());
                return Ok(outcome(&p, model, text, thinking, m, true));
            }
        };
        let bytes = match chunk {
            Err(_) => return Err("The model stopped responding".into()),
            Ok(Err(e)) => return Err(ai::transport_error(e, &redactor)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => bytes,
        };
        for line in lines.push(&bytes) {
            handle(&mut stream, &line, &mut text, &mut thinking, &mut ttft_ms);
        }
        if stream.error.is_some() {
            break;
        }
    }
    if let Some(rest) = lines.finish() {
        handle(&mut stream, &rest, &mut text, &mut thinking, &mut ttft_ms);
    }
    if let Some(e) = &stream.error {
        return Err(AppError::Msg(redactor.clean(&format!("The model returned an error: {e}"))));
    }
    if !stream.done && text.is_empty() && thinking.is_empty() {
        return Err("The connection closed before the model produced anything".into());
    }
    let m = metrics(&p.kind, &stream, ttft_ms, elapsed(started));
    let _ = state.db.conn().execute(
        "INSERT INTO ai_usage (at, provider_id, model, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![db::now(), p.id, model, m.input_tokens.unwrap_or(0) as i64, m.output_tokens.unwrap_or(0) as i64],
    );
    Ok(outcome(&p, model, text, thinking, m, false))
}

#[derive(Serialize, Clone, Default)]
struct InferEvent {
    id: String,
    /// "delta", "done" or "error".
    kind: &'static str,
    text: Option<String>,
    thinking: Option<String>,
    outcome: Option<Outcome>,
    message: Option<String>,
}

/// Start a streamed request. Returns once it is accepted; text and the result arrive as
/// `nexus://lab-infer` events carrying the request's id.
#[tauri::command]
pub fn lab_infer_start<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, request: InferRequest) -> Result<()> {
    validate(&request)?;
    provider_for(&state, &request)?;
    let mut cancel = state.lab.infer.register(&request.id)?;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let id = request.id.clone();
        let emitter = app.clone();
        let event_id = id.clone();
        let mut on_piece = move |piece: &Piece| {
            let mut event = InferEvent { id: event_id.clone(), kind: "delta", ..Default::default() };
            match piece {
                Piece::Text(t) => event.text = Some(t.clone()),
                Piece::Thinking(t) => event.thinking = Some(t.clone()),
            }
            let _ = emitter.emit(EVENT_INFER, event);
        };
        let result = run(&state, &request, &mut cancel, &mut on_piece).await;
        state.lab.infer.finish(&id);
        let event = match result {
            Ok(outcome) => InferEvent { id, kind: "done", outcome: Some(outcome), ..Default::default() },
            Err(e) => InferEvent { id, kind: "error", message: Some(e.to_string()), ..Default::default() },
        };
        let _ = app.emit(EVENT_INFER, event);
    });
    Ok(())
}

#[tauri::command]
pub fn lab_infer_cancel(state: State<'_, AppState>, id: String) -> bool {
    state.lab.infer.cancel(&id)
}

// ------------------------------------------------------------------ saved sessions

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub system: String,
    pub params: GenParams,
    /// The conversation as the Playground shows it: an array of turns with their measurements.
    pub messages: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub turns: usize,
    pub updated_at: String,
}

#[derive(Deserialize, Debug)]
pub struct SessionInput {
    pub id: Option<String>,
    pub title: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub params: GenParams,
    pub messages: Value,
}

pub fn save_session(state: &AppState, input: SessionInput) -> Result<Session> {
    let turns = input.messages.as_array().ok_or("A session is a list of messages")?;
    if turns.is_empty() {
        return Err("There is nothing to save yet".into());
    }
    let messages = serde_json::to_string(&input.messages)?;
    if messages.len() > MAX_SESSION_BYTES || input.system.len() > MAX_PROMPT_BYTES {
        return Err("This session is too large to save".into());
    }
    input.params.validate()?;
    let first_prompt = turns
        .iter()
        .find(|t| t.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let title: String = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| first_prompt.lines().next().unwrap_or("").trim())
        .chars()
        .take(80)
        .collect();
    let title = if title.is_empty() { "Untitled session".to_string() } else { title };
    let id = input.id.filter(|i| !i.trim().is_empty()).unwrap_or_else(db::new_id);
    let now = db::now();
    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO lab_sessions (id, title, provider_id, model, system, params, messages, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, provider_id = excluded.provider_id, model = excluded.model,
             system = excluded.system, params = excluded.params, messages = excluded.messages, updated_at = excluded.updated_at",
        params![id, title, input.provider_id, input.model, input.system, serde_json::to_string(&input.params)?, messages, now],
    )?;
    drop(conn);
    load_session(state, &id)
}

pub fn load_session(state: &AppState, id: &str) -> Result<Session> {
    state
        .db
        .conn()
        .query_row(
            "SELECT id, title, provider_id, model, system, params, messages, created_at, updated_at FROM lab_sessions WHERE id = ?1",
            [id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    provider_id: r.get(2)?,
                    model: r.get(3)?,
                    system: r.get(4)?,
                    params: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                    messages: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or(Value::Array(vec![])),
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Msg("That session no longer exists".into()))
}

#[tauri::command]
pub fn lab_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>> {
    let conn = state.db.conn();
    let mut stmt =
        conn.prepare("SELECT id, title, provider_id, model, messages, updated_at FROM lab_sessions ORDER BY updated_at DESC, rowid DESC")?;
    let rows = stmt.query_map([], |r| {
        let messages: String = r.get(4)?;
        Ok(SessionSummary {
            id: r.get(0)?,
            title: r.get(1)?,
            provider_id: r.get(2)?,
            model: r.get(3)?,
            turns: serde_json::from_str::<Value>(&messages).ok().and_then(|v| v.as_array().map(|a| a.len())).unwrap_or(0),
            updated_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn lab_session_get(state: State<'_, AppState>, id: String) -> Result<Session> {
    load_session(&state, &id)
}

#[tauri::command]
pub fn lab_session_save(state: State<'_, AppState>, session: SessionInput) -> Result<Session> {
    save_session(&state, session)
}

#[tauri::command]
pub fn lab_session_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    state.db.conn().execute("DELETE FROM lab_sessions WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testserver::{serve, Resp};
    use super::*;

    fn user(text: &str) -> Vec<ChatMessage> {
        vec![ChatMessage { role: "user".into(), content: text.into() }]
    }

    fn request(provider: &str, prompt: &str) -> InferRequest {
        InferRequest {
            id: db::new_id(),
            provider_id: provider.into(),
            model: "m:1".into(),
            system: None,
            messages: user(prompt),
            params: GenParams::default(),
            allow_remote: false,
        }
    }

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

    async fn collect(state: &AppState, req: &InferRequest) -> (Result<Outcome>, Vec<Piece>) {
        let (_tx, mut cancel) = watch::channel(false);
        let mut pieces = vec![];
        let result = run(state, req, &mut cancel, &mut |p| pieces.push(p.clone())).await;
        (result, pieces)
    }

    #[test]
    fn request_bodies_carry_only_what_each_runtime_accepts() {
        let all = GenParams {
            temperature: Some(0.0),
            top_p: Some(0.9),
            top_k: Some(40),
            max_tokens: Some(64),
            seed: Some(7),
            repeat_penalty: Some(1.1),
            think: Some(false),
        };
        let msgs = user("hi");
        let ollama = build_body("ollama", "m", Some(" be brief "), &msgs, &all);
        assert_eq!(
            ollama["options"],
            json!({"temperature": 0.0, "top_p": 0.9, "top_k": 40, "num_predict": 64, "seed": 7, "repeat_penalty": 1.1})
        );
        assert_eq!((ollama["think"].as_bool(), ollama["stream"].as_bool()), (Some(false), Some(true)));
        assert_eq!(ollama["messages"], json!([{"role": "system", "content": "be brief"}, {"role": "user", "content": "hi"}]));

        let bare = build_body("ollama", "m", None, &msgs, &GenParams::default());
        assert!(
            bare.get("options").is_none() && bare.get("think").is_none(),
            "unset settings are not sent, so the runtime's defaults apply"
        );

        let openai = build_body("openai", "m", None, &msgs, &all);
        assert_eq!(
            (openai["temperature"].as_f64(), openai["max_tokens"].as_u64(), openai["seed"].as_i64()),
            (Some(0.0), Some(64), Some(7))
        );
        assert!(openai.get("top_k").is_none() && openai.get("repeat_penalty").is_none() && openai.get("think").is_none());
        assert_eq!(openai["stream_options"], json!({"include_usage": true}));
        let llama = build_body("llamacpp", "m", None, &msgs, &all);
        assert_eq!((llama["top_k"].as_u64(), llama["repeat_penalty"].as_f64()), (Some(40), Some(1.1)));

        let claude = build_body("anthropic", "claude-opus-5-5", Some("be brief"), &msgs, &all);
        assert_eq!(
            claude,
            json!({"model": "claude-opus-5-5", "max_tokens": 64, "messages": [{"role": "user", "content": "hi"}], "stream": true, "system": "be brief"})
        );
        assert_eq!(build_body("anthropic", "m", None, &msgs, &GenParams::default())["max_tokens"], 64_000);
    }

    #[test]
    fn ollama_stream_lines_are_parsed_as_the_real_runtime_sends_them() {
        // Captured from Ollama 0.40 with a thinking model.
        let lines = [
            r#"{"model":"qwen3.8:27b","created_at":"2026-11-15T17:33:21.262366Z","message":{"role":"assistant","content":"","thinking":"The"},"done":false}"#,
            r#"{"model":"qwen3.8:27b","created_at":"2026-11-15T17:33:21.262383Z","message":{"role":"assistant","content":"","thinking":" user"},"done":false}"#,
            r#"{"model":"qwen3.8:27b","created_at":"2026-11-15T17:33:09.558925Z","message":{"role":"assistant","content":"1"},"done":false}"#,
            r#"{"model":"qwen3.8:27b","created_at":"2026-11-15T17:33:09.651257Z","message":{"role":"assistant","content":" 2"},"done":false}"#,
            r#"{"model":"qwen3.8:27b","created_at":"2026-11-15T17:33:09.915206Z","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":32838628542,"load_duration":11028325500,"prompt_eval_count":26,"prompt_eval_cached_count":0,"prompt_eval_duration":21035183125,"eval_count":9,"eval_duration":771460708}"#,
        ];
        let mut s = StreamState::default();
        let pieces: Vec<Piece> = lines.iter().flat_map(|l| s.ollama_line(l)).collect();
        assert_eq!(
            pieces,
            [Piece::Thinking("The".into()), Piece::Thinking(" user".into()), Piece::Text("1".into()), Piece::Text(" 2".into())]
        );
        assert!(s.done);
        let m = metrics("ollama", &s, Some(31000.0), 32860.0);
        assert_eq!((m.input_tokens, m.output_tokens, m.stop_reason.as_deref()), (Some(26), Some(9), Some("stop")));
        assert!((m.load_ms.unwrap() - 11028.3255).abs() < 0.001);
        assert!((m.prompt_eval_ms.unwrap() - 21035.183125).abs() < 0.001);
        assert!(
            (m.tokens_per_second.unwrap() - 9.0 / 0.771460708).abs() < 1e-6,
            "tokens per second uses the runtime's own generation time"
        );
        assert_eq!(m.tps_source.as_deref(), Some("runtime"));

        let mut failed = StreamState::default();
        assert!(failed.ollama_line(r#"{"error":"model 'nope' not found"}"#).is_empty());
        assert_eq!(failed.error.as_deref(), Some("model 'nope' not found"));
    }

    #[test]
    fn openai_stream_lines_include_reasoning_usage_and_llama_cpp_timings() {
        // Captured from Ollama's OpenAI-compatible endpoint, which reports llama.cpp-style timings.
        let lines = [
            r#"data: {"id":"chatcmpl-463","object":"chat.completion.chunk","created":1794764002,"model":"qwen3.8:27b","system_fingerprint":"fp_ollama","choices":[{"index":0,"delta":{"content":"","reasoning":" The"},"finish_reason":null}]}"#,
            "",
            r#"data: {"id":"chatcmpl-463","choices":[{"index":0,"delta":{"role":"assistant","content":"Hi"},"finish_reason":null}]}"#,
            ": keep-alive",
            r#"data: {"id":"chatcmpl-463","choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#,
            r#"data: {"id":"chatcmpl-463","choices":[],"usage":{"prompt_tokens":16,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens":16,"total_tokens":32},"timings":{"prompt_n":16,"prompt_ms":176,"prompt_per_token_ms":11,"prompt_per_second":90.9090909090909,"predicted_n":16,"predicted_ms":492,"predicted_per_token_ms":30.75,"predicted_per_second":32.520325203252035}}"#,
            "data: [DONE]",
        ];
        let mut s = StreamState::default();
        let pieces: Vec<Piece> = lines.iter().flat_map(|l| s.openai_line(l)).collect();
        assert_eq!(pieces, [Piece::Thinking(" The".into()), Piece::Text("Hi".into())]);
        assert!(s.done);
        let m = metrics("openai", &s, Some(200.0), 700.0);
        assert_eq!((m.input_tokens, m.output_tokens, m.stop_reason.as_deref()), (Some(16), Some(16), Some("length")));
        assert_eq!((m.prompt_eval_ms, m.eval_ms), (Some(176.0), Some(492.0)));
        assert_eq!((m.tokens_per_second, m.tps_source.as_deref()), (Some(32.520325203252035), Some("runtime")));
        assert_eq!(m.load_ms, None, "this API does not report load time, so none is shown");

        // A server that reports usage but no timings: speed is derived from the client's clock and says so.
        let mut plain = StreamState::default();
        plain.openai_line(r#"data: {"choices":[{"delta":{"content":"a"}}]}"#);
        plain.openai_line(r#"data: {"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":40}}"#);
        let derived = metrics("openai", &plain, Some(500.0), 2500.0);
        assert_eq!((derived.tokens_per_second, derived.tps_source.as_deref()), (Some(20.0), Some("derived")));
        // A server that reports nothing: every count stays unknown rather than zero.
        let mut silent = StreamState::default();
        silent.openai_line(r#"data: {"choices":[{"delta":{"content":"a"}}]}"#);
        let unknown = metrics("openai", &silent, Some(500.0), 2500.0);
        assert_eq!((unknown.input_tokens, unknown.output_tokens, unknown.tokens_per_second, unknown.tps_source), (None, None, None, None));

        let mut failed = StreamState::default();
        failed.openai_line(r#"data: {"error":{"message":"context length exceeded","type":"invalid_request_error"}}"#);
        assert_eq!(failed.error.as_deref(), Some("context length exceeded"));
    }

    #[test]
    fn anthropic_stream_events_are_parsed() {
        let lines = [
            "event: message_start",
            r#"data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"usage":{"input_tokens":12,"cache_read_input_tokens":3,"output_tokens":1}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Considering."}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"abc"}}"#,
            r#"data: {"type":"ping"}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hello"}}"#,
            r#"data: {"type":"content_block_stop","index":1}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
            r#"data: {"type":"message_stop"}"#,
        ];
        let mut s = StreamState::default();
        let pieces: Vec<Piece> = lines.iter().flat_map(|l| s.anthropic_line(l)).collect();
        assert_eq!(pieces, [Piece::Thinking("Considering.".into()), Piece::Text("Hello".into())]);
        assert_eq!((s.done, s.input_tokens, s.output_tokens, s.stop_reason.as_deref()), (true, Some(15), Some(12), Some("end_turn")));

        let mut failed = StreamState::default();
        failed.anthropic_line(r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#);
        assert_eq!(failed.error.as_deref(), Some("Overloaded"));
        let mut refused = StreamState::default();
        refused.anthropic_line(r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":0}}"#);
        assert_eq!(refused.stop_reason.as_deref(), Some("refusal"));
    }

    #[tokio::test]
    async fn a_streamed_request_measures_time_to_first_token_and_reports_runtime_counts() {
        let server = serve(|req| match req.path.as_str() {
            "/api/chat" => Resp::stream("application/x-ndjson", vec![
                (120, "{\"message\":{\"role\":\"assistant\",\"content\":\"Hel\"},\"done\":false}\n".into()),
                (40, "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n{\"message\":{\"role\":\"assist".into()),
                (40, "ant\",\"content\":\"!\"},\"done\":false}\n".into()),
                (0, "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"load_duration\":5000000,\"prompt_eval_count\":4,\"prompt_eval_duration\":20000000,\"eval_count\":3,\"eval_duration\":100000000}\n".into()),
            ]),
            _ => Resp::error(404, json!({"error": "nope"})),
        }).await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        let mut req = request("o", "say hello");
        req.params.temperature = Some(0.0);
        let (result, pieces) = collect(&state, &req).await;
        let out = result.unwrap();
        assert_eq!((out.text.as_str(), out.cancelled, out.runtime.as_str(), out.local), ("Hello!", false, "ollama", true));
        assert_eq!(pieces.len(), 3, "text arrives piece by piece, including one split across network chunks");
        let m = &out.metrics;
        let ttft = m.ttft_ms.unwrap();
        assert!((100.0..2000.0).contains(&ttft), "first token after the server's 120 ms delay, got {ttft}");
        assert!(m.total_ms >= ttft + 60.0, "total {} covers the later pieces", m.total_ms);
        assert_eq!((m.input_tokens, m.output_tokens, m.load_ms, m.eval_ms), (Some(4), Some(3), Some(5.0), Some(100.0)));
        assert_eq!(m.tokens_per_second, Some(30.0));
        let sent = server.last("/api/chat").json();
        assert_eq!(
            (sent["model"].as_str(), sent["options"]["temperature"].as_f64(), sent["stream"].as_bool()),
            (Some("m:1"), Some(0.0), Some(true))
        );
        let usage: (i64, i64) =
            state.db.conn().query_row("SELECT input_tokens, output_tokens FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(usage, (4, 3));
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn cancelling_a_stream_closes_the_connection_and_keeps_what_was_generated() {
        let server = serve(|_| {
            Resp::stream_then_hang("application/x-ndjson", vec![(0, "{\"message\":{\"content\":\"partial\"},\"done\":false}\n".into())])
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        let req = request("o", "write a long story");
        let (tx, mut cancel) = watch::channel(false);
        let mut seen = 0;
        let started = Instant::now();
        let result = run(&state, &req, &mut cancel, &mut |_| {
            seen += 1;
            let _ = tx.send(true); // stop as soon as the first text arrives
        })
        .await
        .unwrap();
        assert_eq!((result.text.as_str(), result.cancelled, result.metrics.stop_reason.as_deref()), ("partial", true, Some("cancelled")));
        assert!(
            result.metrics.ttft_ms.is_some() && result.metrics.output_tokens.is_none(),
            "no token count is invented for a stopped request"
        );
        assert!(started.elapsed() < Duration::from_secs(5), "cancelling does not wait for the model");
        // The runtime sees the client go away, which is what makes it stop generating.
        let waited = Instant::now();
        while server.disconnects.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            assert!(waited.elapsed() < Duration::from_secs(5), "the connection was not closed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let rows: i64 = state.db.conn().query_row("SELECT COUNT(*) FROM ai_usage", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn failures_are_reported_plainly_and_without_secrets() {
        let server = serve(|req| match req.path.as_str() {
            "/api/chat" => Resp::error(404, json!({"error": "model 'm:1' not found"})),
            "/v1/chat/completions" => Resp::error(401, json!({"error": {"message": "Incorrect API key provided: sk-abcdef1234567890. Find it at https://user:pw@example.com/keys?token=xyz"}})),
            _ => Resp::stream("text/event-stream", vec![(0, "data: {\"error\":{\"message\":\"backend crashed\"}}\n\n".into())]),
        }).await;
        let state = AppState::for_tests();
        add_provider(&state, "o", "ollama", &server.url);
        add_provider(&state, "g", "openai", &format!("{}/v1", server.url));
        add_provider(&state, "s", "openai", &format!("{}/other", server.url));
        add_provider(&state, "dead", "ollama", "http://127.0.0.1:9");
        add_provider(&state, "off", "ollama", &server.url);
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'off'", []).unwrap();

        let err = |r: (Result<Outcome>, Vec<Piece>)| r.0.unwrap_err().to_string();
        assert_eq!(err(collect(&state, &request("o", "hi")).await), "Provider returned 404 Not Found: model 'm:1' not found");
        let leaked = err(collect(&state, &request("g", "hi")).await);
        assert!(leaked.contains("401") && !leaked.contains("sk-abcdef") && !leaked.contains("pw@") && !leaked.contains("xyz"), "{leaked}");
        assert!(err(collect(&state, &request("s", "hi")).await).contains("backend crashed"));
        assert!(err(collect(&state, &request("dead", "hi")).await).contains("Could not connect"));
        assert!(err(collect(&state, &request("off", "hi")).await).contains("disabled"));
        assert!(err(collect(&state, &request("missing", "hi")).await).contains("not found"));
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn nothing_is_sent_off_this_machine_without_confirmation() {
        let state = AppState::for_tests();
        add_provider(&state, "remote", "openai", "https://api.example.invalid/v1");
        let mut req = request("remote", "secret project details");
        let refused = collect(&state, &req).await.0.unwrap_err().to_string();
        assert!(refused.contains("not on this machine") && refused.contains("api.example.invalid"), "{refused}");
        // With confirmation the request is attempted (and here fails to connect, as the host does not exist).
        req.allow_remote = true;
        let attempted = collect(&state, &req).await.0.unwrap_err().to_string();
        assert!(!attempted.contains("not on this machine"), "{attempted}");
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn requests_are_validated_before_anything_is_sent() {
        let ok = request("p", "hello");
        validate(&ok).unwrap();
        let mut bad = vec![];
        bad.push(InferRequest { id: "short".into(), ..ok.clone() });
        bad.push(InferRequest { id: "../../../etc/passwd".into(), ..ok.clone() });
        bad.push(InferRequest { model: "  ".into(), ..ok.clone() });
        bad.push(InferRequest { messages: vec![], ..ok.clone() });
        bad.push(InferRequest { messages: user("   "), ..ok.clone() });
        bad.push(InferRequest { messages: vec![ChatMessage { role: "system".into(), content: "x".into() }], ..ok.clone() });
        bad.push(InferRequest { messages: vec![ChatMessage { role: "assistant".into(), content: "x".into() }], ..ok.clone() });
        bad.push(InferRequest { messages: user(&"x".repeat(MAX_PROMPT_BYTES + 1)), ..ok.clone() });
        bad.push(InferRequest { params: GenParams { temperature: Some(9.0), ..Default::default() }, ..ok.clone() });
        for req in bad {
            assert!(validate(&req).is_err(), "{:?}", (&req.id, &req.model, req.messages.len()));
        }
    }

    #[test]
    fn sessions_are_saved_reopened_and_updated_in_place() {
        let state = AppState::for_tests();
        let turns = json!([{"role": "user", "content": "Explain borrow checking\nin Rust"}, {"role": "assistant", "content": "It…", "metrics": {"total_ms": 1200.0, "output_tokens": null}}]);
        let saved = save_session(
            &state,
            SessionInput {
                id: None,
                title: None,
                provider_id: Some("p".into()),
                model: Some("m:1".into()),
                system: "be brief".into(),
                params: GenParams { temperature: Some(0.3), ..Default::default() },
                messages: turns.clone(),
            },
        )
        .unwrap();
        assert_eq!(saved.title, "Explain borrow checking", "the first prompt line names the session");
        let reopened = load_session(&state, &saved.id).unwrap();
        assert_eq!(
            (reopened.messages.clone(), reopened.system.as_str(), reopened.params.temperature),
            (turns.clone(), "be brief", Some(0.3))
        );
        assert!(reopened.messages[1]["metrics"]["output_tokens"].is_null(), "unknown measurements stay unknown when reloaded");

        let renamed = save_session(
            &state,
            SessionInput {
                id: Some(saved.id.clone()),
                title: Some("  Borrowing  ".into()),
                provider_id: None,
                model: None,
                system: String::new(),
                params: GenParams::default(),
                messages: turns,
            },
        )
        .unwrap();
        assert_eq!(
            (renamed.id.as_str(), renamed.title.as_str(), renamed.created_at.as_str()),
            (saved.id.as_str(), "Borrowing", saved.created_at.as_str())
        );
        let count: i64 = state.db.conn().query_row("SELECT COUNT(*) FROM lab_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);

        for bad in [json!([]), json!({"not": "a list"}), json!("text")] {
            assert!(save_session(
                &state,
                SessionInput {
                    id: None,
                    title: None,
                    provider_id: None,
                    model: None,
                    system: String::new(),
                    params: GenParams::default(),
                    messages: bad
                }
            )
            .is_err());
        }
        assert!(load_session(&state, "missing").is_err());
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }
}
