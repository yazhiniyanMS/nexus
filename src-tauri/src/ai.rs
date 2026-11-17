//! AI workspace: provider configuration, secure credential storage, model discovery and chat.
//! Everything here is optional; the rest of NEXUS works with no provider configured.

use crate::error::{AppError, Result};
use crate::{activity, db, platform, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::State;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// Output cap for non-streamed Anthropic requests; large enough that thinking does not starve the answer.
const ANTHROPIC_MAX_TOKENS: u32 = 16_000;

#[derive(Serialize, Clone, Debug)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// "openai" (any OpenAI-compatible API), "llamacpp" (a llama.cpp server), "anthropic" or "ollama".
    pub kind: String,
    /// For "openai" this includes the API prefix (…/v1); for the others it is the server root.
    pub base_url: String,
    pub default_model: Option<String>,
    pub has_key: bool,
    /// A disabled connection is kept but never contacted.
    pub enabled: bool,
    /// True when the endpoint is on this machine (loopback), so requests do not leave it.
    pub local: bool,
}

pub const KINDS: [&str; 4] = ["ollama", "llamacpp", "openai", "anthropic"];

#[derive(Deserialize)]
pub struct ProviderInput {
    id: Option<String>,
    name: String,
    kind: String,
    base_url: String,
    default_model: Option<String>,
    /// None leaves the stored key untouched; an empty string removes it.
    api_key: Option<String>,
    /// None keeps the current state (new connections start enabled).
    enabled: Option<bool>,
}

/// True when a URL points at this machine. Anything else means a request leaves it.
pub fn is_local_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    let Some(host) = parsed.host_str() else { return false };
    let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback() || ip.is_unspecified(),
        Err(_) => host == "localhost" || host.ends_with(".localhost"),
    }
}

/// Check and normalise a base URL: http(s) only, a host, and nothing that could carry a secret
/// (no user:password@, query or fragment), so URLs are safe to show and log.
pub fn normalize_base_url(kind: &str, raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| AppError::Msg("Base URL is not a valid URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Base URL must start with http:// or https://".into());
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err("Base URL needs a host name".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Do not put credentials in the URL. Use the API key field; it is stored in the keychain.".into());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Base URL must not contain a query string or fragment".into());
    }
    let mut url = trimmed.to_string();
    // A llama.cpp server is addressed by its root; the OpenAI-style prefix is added per request.
    if kind == "llamacpp" {
        url = url.trim_end_matches("/v1").trim_end_matches('/').to_string();
    }
    Ok(url)
}

/// Base of the OpenAI-style API for a provider.
pub(crate) fn openai_base(p: &Provider) -> String {
    if p.kind == "llamacpp" {
        format!("{}/v1", p.base_url)
    } else {
        p.base_url.clone()
    }
}

/// Remove anything secret-looking from text that may be shown or logged: the given key,
/// credentials or query strings inside URLs, bearer tokens and "sk-" style keys.
pub fn redact(text: &str, secret: Option<&str>) -> String {
    static PATTERNS: std::sync::OnceLock<[regex::Regex; 3]> = std::sync::OnceLock::new();
    let [url, bearer, key] = PATTERNS.get_or_init(|| {
        [
            regex::Regex::new(r"(?i)(https?://)(?:[^/\s@]+@)?([^\s?#]*)(\?[^\s#]*)?").unwrap(),
            regex::Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{6,}").unwrap(),
            regex::Regex::new(r"\bsk-[A-Za-z0-9_-]{6,}").unwrap(),
        ]
    });
    let mut out = text.to_string();
    if let Some(secret) = secret.map(str::trim).filter(|s| s.len() >= 4) {
        out = out.replace(secret, "[redacted]");
    }
    let out =
        url.replace_all(&out, |c: &regex::Captures| format!("{}{}{}", &c[1], &c[2], if c.get(3).is_some() { "?[redacted]" } else { "" }));
    let out = bearer.replace_all(&out, "${1}[redacted]");
    key.replace_all(&out, "[redacted]").into_owned()
}

fn read_key(id: &str) -> Result<Option<String>> {
    platform::secrets::get(id)
}

fn row_to_provider(r: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    let base_url: String = r.get(3)?;
    Ok(Provider {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        local: is_local_url(&base_url),
        base_url,
        default_model: r.get(4)?,
        has_key: r.get::<_, i64>(5)? != 0,
        enabled: r.get::<_, i64>(6)? != 0,
    })
}

const COLS: &str = "id, name, kind, base_url, default_model, has_key, enabled";

pub(crate) fn get_provider(state: &AppState, id: &str) -> Result<Provider> {
    state
        .db
        .conn()
        .query_row(&format!("SELECT {COLS} FROM providers WHERE id = ?1"), [id], row_to_provider)
        .optional()?
        .ok_or_else(|| "Provider not found".into())
}

/// A provider that may be contacted: it exists and is enabled.
pub(crate) fn usable_provider(state: &AppState, id: &str) -> Result<Provider> {
    let p = get_provider(state, id)?;
    if !p.enabled {
        return Err(AppError::Msg(format!("The connection “{}” is disabled. Enable it in Model Lab → Providers to use it.", p.name)));
    }
    Ok(p)
}

/// A provider that project content may be sent to: usable, and either on this machine or
/// explicitly allowed by the user for this request.
pub(crate) fn provider_for_project_data(state: &AppState, id: &str, allow_remote: bool) -> Result<Provider> {
    let p = usable_provider(state, id)?;
    if !p.local && !allow_remote {
        let host = reqwest::Url::parse(&p.base_url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| p.base_url.clone());
        return Err(AppError::Msg(format!(
            "“{}” is not on this machine: project content would be sent to {host}. Confirm that first.",
            p.name
        )));
    }
    Ok(p)
}

pub(crate) fn all_providers(state: &AppState) -> Result<Vec<Provider>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM providers ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_provider)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<Provider>> {
    all_providers(&state)
}

#[tauri::command]
pub fn save_provider(state: State<'_, AppState>, input: ProviderInput) -> Result<Provider> {
    if !KINDS.contains(&input.kind.as_str()) {
        return Err("Unknown provider type".into());
    }
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Name is required".into());
    }
    if name.chars().count() > 80 {
        return Err("Name is too long".into());
    }
    let base_url = normalize_base_url(&input.kind, &input.base_url)?;
    let id = input.id.unwrap_or_else(db::new_id);
    let model = input.default_model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());

    // Credentials go to the OS keychain only; SQLite stores just a "has key" flag.
    let mut has_key = read_key(&id).unwrap_or(None).is_some();
    if let Some(key) = input.api_key {
        let key = key.trim();
        if key.is_empty() {
            platform::secrets::delete(&id)?;
            has_key = false;
        } else {
            platform::secrets::set(&id, key)?;
            has_key = true;
        }
    }
    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, COALESCE(?8, 1))
         ON CONFLICT(id) DO UPDATE SET name = ?2, kind = ?3, base_url = ?4, default_model = ?5, has_key = ?6,
             enabled = COALESCE(?8, enabled)",
        params![id, name, input.kind, base_url, model, has_key as i64, db::now(), input.enabled.map(|e| e as i64)],
    )?;
    activity::log(&conn, "ai", &format!("Saved AI provider {name}"), None);
    Ok(conn.query_row(&format!("SELECT {COLS} FROM providers WHERE id = ?1"), [&id], row_to_provider)?)
}

/// Turn a connection off without losing its settings or key, or back on.
#[tauri::command]
pub fn set_provider_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> Result<Provider> {
    let conn = state.db.conn();
    if conn.execute("UPDATE providers SET enabled = ?2 WHERE id = ?1", params![id, enabled as i64])? == 0 {
        return Err("Provider not found".into());
    }
    let p = conn.query_row(&format!("SELECT {COLS} FROM providers WHERE id = ?1"), [&id], row_to_provider)?;
    activity::log(&conn, "ai", &format!("{} AI provider {}", if enabled { "Enabled" } else { "Disabled" }, p.name), None);
    Ok(p)
}

#[tauri::command]
pub fn delete_provider(state: State<'_, AppState>, id: String) -> Result<()> {
    let _ = platform::secrets::delete(&id);
    state.db.conn().execute("DELETE FROM providers WHERE id = ?1", [&id])?;
    Ok(())
}

/// Remembers a provider's key only so it can be removed from anything shown or logged.
#[derive(Default)]
pub(crate) struct Redactor(Option<String>);

impl Redactor {
    pub fn clean(&self, text: &str) -> String {
        redact(text, self.0.as_deref())
    }
}

/// A request with the provider's credentials attached.
pub(crate) struct Authed {
    pub req: reqwest::RequestBuilder,
    pub redactor: Redactor,
}

/// Attach the provider's authentication headers to a request.
pub(crate) fn authed(provider: &Provider, req: reqwest::RequestBuilder) -> Result<Authed> {
    let key = if provider.has_key { read_key(&provider.id)? } else { None };
    let req = match (provider.kind.as_str(), &key) {
        ("anthropic", Some(k)) => req.header("x-api-key", k).header("anthropic-version", "2023-06-01"),
        ("anthropic", None) => return Err("This provider needs an API key".into()),
        (_, Some(k)) => req.bearer_auth(k),
        (_, None) => req,
    };
    Ok(Authed { req, redactor: Redactor(key) })
}

/// Turn a transport failure into a message a person can act on, with nothing secret in it.
pub(crate) fn transport_error(e: reqwest::Error, redactor: &Redactor) -> AppError {
    if e.is_connect() {
        AppError::Msg("Could not connect. Check the base URL and that the server is running.".into())
    } else if e.is_timeout() {
        AppError::Msg("The request timed out".into())
    } else {
        AppError::Msg(redactor.clean(&format!("Network error: {e}")))
    }
}

/// The provider's own explanation from an error body, or the start of the body.
pub(crate) fn error_detail(status: reqwest::StatusCode, body: &str, redactor: &Redactor) -> AppError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let e = v.get("error")?;
            e.get("message").and_then(|m| m.as_str()).or(e.as_str()).map(String::from)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    AppError::Msg(redactor.clean(&format!("Provider returned {status}: {detail}")))
}

pub(crate) async fn send_json_within(a: Authed, timeout: Duration) -> Result<Value> {
    let Authed { req, redactor } = a;
    let resp = req.timeout(timeout).send().await.map_err(|e| transport_error(e, &redactor))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| transport_error(e, &redactor))?;
    if !status.is_success() {
        return Err(error_detail(status, &text, &redactor));
    }
    serde_json::from_str(&text).map_err(|_| AppError::Msg("The server replied with something that is not JSON. Check the base URL.".into()))
}

pub(crate) async fn send_json(a: Authed) -> Result<Value> {
    send_json_within(a, REQUEST_TIMEOUT).await
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelInfo {
    pub(crate) id: String,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) family: Option<String>,
    pub(crate) parameter_size: Option<String>,
    pub(crate) quantization: Option<String>,
    pub(crate) modified_at: Option<String>,
}

pub(crate) fn parse_ollama_tags(v: &Value) -> Vec<ModelInfo> {
    let s = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    v.get("models")
        .and_then(|m| m.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| {
                    let d = m.get("details").cloned().unwrap_or(Value::Null);
                    Some(ModelInfo {
                        id: s(m, "name")?,
                        size_bytes: m.get("size").and_then(|x| x.as_u64()),
                        family: s(&d, "family"),
                        parameter_size: s(&d, "parameter_size"),
                        quantization: s(&d, "quantization_level"),
                        modified_at: s(m, "modified_at"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_model_list(v: &Value) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|m| {
                    Some(ModelInfo {
                        id: m.get("id")?.as_str()?.to_string(),
                        size_bytes: None,
                        family: m.get("owned_by").and_then(|x| x.as_str()).map(String::from),
                        parameter_size: None,
                        quantization: None,
                        modified_at: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// How long a listing or health request may take; these should answer quickly.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) async fn fetch_models(state: &AppState, p: &Provider) -> Result<Vec<ModelInfo>> {
    let get = |url: String| authed(p, state.http.get(url));
    match p.kind.as_str() {
        "ollama" => {
            let v = send_json_within(get(format!("{}/api/tags", p.base_url))?, PROBE_TIMEOUT).await?;
            Ok(parse_ollama_tags(&v))
        }
        "anthropic" => {
            let v = send_json_within(get(format!("{}/v1/models?limit=100", p.base_url))?, PROBE_TIMEOUT).await?;
            Ok(parse_model_list(&v))
        }
        _ => {
            let v = send_json_within(get(format!("{}/models", openai_base(p)))?, PROBE_TIMEOUT).await?;
            Ok(parse_model_list(&v))
        }
    }
}

/// Lists models for a provider.
#[tauri::command]
pub async fn list_models(state: State<'_, AppState>, provider_id: String) -> Result<Vec<ModelInfo>> {
    let p = usable_provider(&state, &provider_id)?;
    fetch_models(&state, &p).await
}

#[derive(Serialize, Debug)]
pub struct ConnectionTest {
    pub ok: bool,
    /// Round-trip time of the first successful request, measured here.
    pub latency_ms: Option<u64>,
    pub version: Option<String>,
    pub models: Option<usize>,
    /// What was found, or why the test failed.
    pub detail: String,
}

/// Contact a provider and report what answered. Never returns an error for an unreachable
/// endpoint: that is a normal test result.
pub(crate) async fn test_connection(state: &AppState, p: &Provider) -> ConnectionTest {
    let failed = |detail: String| ConnectionTest { ok: false, latency_ms: None, version: None, models: None, detail };
    if !p.enabled {
        return failed("This connection is disabled.".into());
    }
    let started = Instant::now();
    let mut version = None;
    let mut ready = None;
    // Runtime-specific health endpoints first, because they answer even with no model installed.
    match p.kind.as_str() {
        "ollama" => match authed(p, state.http.get(format!("{}/api/version", p.base_url))) {
            Ok(a) => match send_json_within(a, PROBE_TIMEOUT).await {
                Ok(v) => version = v.get("version").and_then(|x| x.as_str()).map(String::from),
                Err(e) => return failed(e.to_string()),
            },
            Err(e) => return failed(e.to_string()),
        },
        "llamacpp" => match authed(p, state.http.get(format!("{}/health", p.base_url))) {
            // 503 with a JSON error body means the server is up but still loading its model.
            Ok(a) => match send_json_within(a, PROBE_TIMEOUT).await {
                Ok(v) => ready = v.get("status").and_then(|x| x.as_str()).map(String::from),
                Err(e) if e.to_string().contains("503") => ready = Some("loading model".into()),
                Err(e) => return failed(e.to_string()),
            },
            Err(e) => return failed(e.to_string()),
        },
        _ => {}
    }
    let latency = started.elapsed().as_millis() as u64;
    match fetch_models(state, p).await {
        Ok(models) => {
            let latency_ms = if version.is_some() || ready.is_some() { latency } else { started.elapsed().as_millis() as u64 };
            let n = models.len();
            let what = match p.kind.as_str() {
                "ollama" => format!("Ollama{}", version.as_deref().map(|v| format!(" {v}")).unwrap_or_default()),
                "llamacpp" => format!("llama.cpp server{}", ready.as_deref().map(|r| format!(" ({r})")).unwrap_or_default()),
                "anthropic" => "Anthropic API".to_string(),
                _ => "OpenAI-compatible endpoint".to_string(),
            };
            ConnectionTest {
                ok: true,
                latency_ms: Some(latency_ms),
                version,
                models: Some(n),
                detail: format!("{what} · {n} model{} listed", if n == 1 { "" } else { "s" }),
            }
        }
        Err(e) => failed(e.to_string()),
    }
}

#[tauri::command]
pub async fn test_provider(state: State<'_, AppState>, id: String) -> Result<ConnectionTest> {
    let p = get_provider(&state, &id)?;
    Ok(test_connection(&state, &p).await)
}

#[derive(Serialize)]
pub struct LocalRuntime {
    name: String,
    kind: String,
    base_url: String,
    running: bool,
    models: Vec<ModelInfo>,
}

/// Probe the default ports of common local model runtimes.
#[tauri::command]
pub async fn detect_local(state: State<'_, AppState>) -> Result<Vec<LocalRuntime>> {
    let probes = [
        ("Ollama", "ollama", "http://localhost:11434", "http://localhost:11434/api/tags"),
        ("LM Studio", "openai", "http://localhost:1234/v1", "http://localhost:1234/v1/models"),
    ];
    let mut out = vec![];
    for (name, kind, base, url) in probes {
        let res = state.http.get(url).timeout(Duration::from_millis(1500)).send().await;
        let (running, models) = match res {
            Ok(r) if r.status().is_success() => {
                let v: Value = r.json().await.unwrap_or(Value::Null);
                (true, if kind == "ollama" { parse_ollama_tags(&v) } else { parse_model_list(&v) })
            }
            _ => (false, vec![]),
        };
        out.push(LocalRuntime { name: name.into(), kind: kind.into(), base_url: base.into(), running, models });
    }
    Ok(out)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Debug)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
}

/// Run one chat completion against a provider and record token usage.
pub async fn complete(
    state: &AppState,
    provider_id: &str,
    model: Option<String>,
    system: Option<String>,
    messages: Vec<ChatMessage>,
) -> Result<ChatResponse> {
    let p = usable_provider(state, provider_id)?;
    let model = model.filter(|m| !m.trim().is_empty()).or_else(|| p.default_model.clone()).ok_or("Choose a model first")?;
    let system = system.filter(|s| !s.trim().is_empty());
    let started = Instant::now();
    let u = |v: &Value, path: &[&str]| path.iter().try_fold(v, |acc, k| acc.get(k)).and_then(|x| x.as_u64());

    let (content, input_tokens, output_tokens) = match p.kind.as_str() {
        "anthropic" => {
            // Sampling parameters are deliberately not sent: current Claude models reject them.
            let mut body = json!({ "model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "messages": messages });
            if let Some(s) = &system {
                body["system"] = json!(s);
            }
            let v = send_json(authed(&p, state.http.post(format!("{}/v1/messages", p.base_url)).json(&body))?).await?;
            if v.get("stop_reason").and_then(|r| r.as_str()) == Some("refusal") {
                return Err("The model declined this request (stop reason: refusal).".into());
            }
            let text = v
                .get("content")
                .and_then(|c| c.as_array())
                .map(|blocks| blocks.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(""))
                .unwrap_or_default();
            (text, u(&v, &["usage", "input_tokens"]), u(&v, &["usage", "output_tokens"]))
        }
        kind => {
            let mut msgs = vec![];
            if let Some(s) = &system {
                msgs.push(ChatMessage { role: "system".into(), content: s.clone() });
            }
            msgs.extend(messages);
            if kind == "ollama" {
                let body = json!({ "model": model, "messages": msgs, "stream": false });
                let v = send_json(authed(&p, state.http.post(format!("{}/api/chat", p.base_url)).json(&body))?).await?;
                let text = v.pointer("/message/content").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                (text, u(&v, &["prompt_eval_count"]), u(&v, &["eval_count"]))
            } else {
                let body = json!({ "model": model, "messages": msgs });
                let v = send_json(authed(&p, state.http.post(format!("{}/chat/completions", openai_base(&p))).json(&body))?).await?;
                let text = v.pointer("/choices/0/message/content").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                (text, u(&v, &["usage", "prompt_tokens"]), u(&v, &["usage", "completion_tokens"]))
            }
        }
    };

    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO ai_usage (at, provider_id, model, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![db::now(), p.id, model, input_tokens.unwrap_or(0) as i64, output_tokens.unwrap_or(0) as i64],
    )?;
    Ok(ChatResponse { content, model, input_tokens, output_tokens, duration_ms: started.elapsed().as_millis() as u64 })
}

/// What the agent engine needs from a model: one completion. Keeping this behind a trait means
/// the engine never sees providers or credentials, and tests can substitute a scripted client.
pub type ModelFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<ChatResponse>> + Send + 'a>>;

pub trait ModelClient: Send + Sync {
    fn complete<'a>(&'a self, state: &'a AppState, system: String, user: String) -> ModelFuture<'a>;
}

/// The real client: a configured provider (Anthropic, OpenAI-compatible or Ollama) and model.
pub struct ProviderClient {
    pub provider_id: String,
    pub model: Option<String>,
}

impl ModelClient for ProviderClient {
    fn complete<'a>(&'a self, state: &'a AppState, system: String, user: String) -> ModelFuture<'a> {
        Box::pin(async move {
            complete(state, &self.provider_id, self.model.clone(), Some(system), vec![ChatMessage { role: "user".into(), content: user }])
                .await
        })
    }
}

#[tauri::command]
pub async fn chat(
    state: State<'_, AppState>,
    provider_id: String,
    model: Option<String>,
    system: Option<String>,
    messages: Vec<ChatMessage>,
) -> Result<ChatResponse> {
    complete(&state, &provider_id, model, system, messages).await
}

#[derive(Serialize)]
pub struct UsageRow {
    provider: String,
    model: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[tauri::command]
pub fn usage_summary(state: State<'_, AppState>) -> Result<Vec<UsageRow>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(
        "SELECT COALESCE(p.name, 'Removed provider'), u.model, COUNT(*), SUM(u.input_tokens), SUM(u.output_tokens)
         FROM ai_usage u LEFT JOIN providers p ON p.id = u.provider_id
         GROUP BY u.provider_id, u.model ORDER BY SUM(u.input_tokens + u.output_tokens) DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(UsageRow { provider: r.get(0)?, model: r.get(1)?, requests: r.get(2)?, input_tokens: r.get(3)?, output_tokens: r.get(4)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ollama_tags() {
        let v = json!({"models":[{"name":"llama3:8b","size":4661224676u64,"modified_at":"2026-01-01T00:00:00Z",
            "details":{"family":"llama","parameter_size":"8.0B","quantization_level":"Q4_0"}}]});
        let m = parse_ollama_tags(&v);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "llama3:8b");
        assert_eq!(m[0].quantization.as_deref(), Some("Q4_0"));
    }

    #[test]
    fn parses_openai_style_model_list() {
        let v = json!({"data":[{"id":"b"},{"id":"a","owned_by":"x"}]});
        let m = parse_model_list(&v);
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn secrets_are_removed_from_anything_shown_or_logged() {
        let key = "sk-ant-FAKE-test-key-not-real";
        assert_eq!(redact(&format!("Incorrect API key provided: {key}."), Some(key)), "Incorrect API key provided: [redacted].");
        // Without knowing the key, key-shaped text, bearer tokens and URL secrets still go.
        assert_eq!(redact("bad key sk-proj-abcdef123456 used", None), "bad key [redacted] used");
        assert_eq!(redact("Authorization: Bearer abc.def-123456 rejected", None), "Authorization: Bearer [redacted] rejected");
        assert_eq!(
            redact("GET https://user:hunter2@api.example.com/v1/models?api_key=abc123&x=1 failed", None),
            "GET https://api.example.com/v1/models?[redacted] failed"
        );
        assert_eq!(
            redact("error sending request for url (http://localhost:11434/api/tags)", None),
            "error sending request for url (http://localhost:11434/api/tags)"
        );
        // A custom key that looks like nothing in particular is removed when it is known.
        assert_eq!(redact("token=my-private-token-42 was refused", Some("my-private-token-42")), "token=[redacted] was refused");
        assert_eq!(
            redact("short key ab is kept", Some("ab")),
            "short key ab is kept",
            "a trivially short key is not treated as a secret to avoid mangling text"
        );
    }

    #[test]
    fn local_and_remote_addresses_are_told_apart() {
        for local in [
            "http://localhost:11434",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:1234",
            "http://LOCALHOST:1",
            "http://app.localhost:3000",
            "http://0.0.0.0:11434",
        ] {
            assert!(is_local_url(local), "{local}");
        }
        for remote in [
            "https://api.anthropic.com",
            "http://192.168.1.20:11434",
            "http://localhost.example.com",
            "http://10.0.0.5:8080",
            "not a url",
            "http://example.com/127.0.0.1",
        ] {
            assert!(!is_local_url(remote), "{remote}");
        }
    }

    #[test]
    fn project_content_only_goes_to_a_remote_model_with_explicit_permission() {
        let state = AppState::for_tests();
        for (id, url) in [("near", "http://localhost:11434"), ("far", "https://api.example.invalid/v1")] {
            state
                .db
                .conn()
                .execute(
                    "INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES (?1, ?1, 'openai', ?2, 0, '')",
                    params![id, url],
                )
                .unwrap();
        }
        assert!(provider_for_project_data(&state, "near", false).is_ok());
        let refused = provider_for_project_data(&state, "far", false).unwrap_err().to_string();
        assert!(refused.contains("not on this machine") && refused.contains("api.example.invalid"), "{refused}");
        assert!(provider_for_project_data(&state, "far", true).is_ok());
        state.db.conn().execute("UPDATE providers SET enabled = 0", []).unwrap();
        assert!(provider_for_project_data(&state, "near", true).unwrap_err().to_string().contains("disabled"));
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn base_urls_are_normalised_and_may_not_carry_credentials() {
        assert_eq!(normalize_base_url("ollama", " http://localhost:11434/ ").unwrap(), "http://localhost:11434");
        assert_eq!(normalize_base_url("openai", "https://api.openai.com/v1/").unwrap(), "https://api.openai.com/v1");
        assert_eq!(
            normalize_base_url("llamacpp", "http://localhost:8080/v1").unwrap(),
            "http://localhost:8080",
            "a llama.cpp server is addressed by its root"
        );
        for bad in [
            "localhost:11434",
            "ftp://host",
            "file:///etc/passwd",
            "https://user:pw@host/v1",
            "https://host/v1?key=abc",
            "https://host/v1#frag",
            "http://",
            "",
        ] {
            assert!(normalize_base_url("openai", bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn connections_can_be_tested_disabled_and_are_never_contacted_while_disabled() {
        use crate::lab::testserver::{serve, Resp};
        let server = serve(|req| match req.path.as_str() {
            "/api/version" => Resp::json(json!({"version": "0.40.16"})),
            "/api/tags" => Resp::json(json!({"models": [{"name": "a:1"}, {"name": "b:1"}]})),
            "/health" => Resp::error(503, json!({"error": {"code": 503, "message": "Loading model", "type": "unavailable_error"}})),
            "/v1/models" => Resp::json(json!({"data": [{"id": "tiny.gguf"}]})),
            _ => Resp::error(404, json!({"error": {"message": "unknown path"}})),
        })
        .await;
        let state = AppState::for_tests();
        let add = |id: &str, kind: &str, url: String| {
            state
                .db
                .conn()
                .execute(
                    "INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES (?1, ?1, ?2, ?3, 0, ?1)",
                    params![id, kind, url],
                )
                .unwrap();
        };
        add("ollama", "ollama", server.url.clone());
        add("llama", "llamacpp", server.url.clone());
        add("generic", "openai", format!("{}/v1", server.url));
        add("wrong", "openai", format!("{}/nope", server.url));
        add("dead", "ollama", "http://127.0.0.1:9".into());
        add("claude", "anthropic", server.url.clone());
        let test = |id: &'static str| {
            let state = &state;
            async move { test_connection(state, &get_provider(state, id).unwrap()).await }
        };

        let o = test("ollama").await;
        assert_eq!(
            (o.ok, o.version.as_deref(), o.models, o.detail.as_str()),
            (true, Some("0.40.16"), Some(2), "Ollama 0.40.16 · 2 models listed")
        );
        assert!(o.latency_ms.is_some());
        let l = test("llama").await;
        assert_eq!((l.ok, l.models, l.detail.as_str()), (true, Some(1), "llama.cpp server (loading model) · 1 model listed"));
        assert!(test("generic").await.ok);
        let wrong = test("wrong").await;
        assert!(!wrong.ok && wrong.detail.contains("404") && wrong.latency_ms.is_none(), "{wrong:?}");
        assert!(test("dead").await.detail.contains("Could not connect"));
        assert!(test("claude").await.detail.contains("needs an API key"), "a missing key is reported, not sent as an empty one");

        // Disabled: reported as such, refused for use, and not contacted at all.
        let p = get_provider(&state, "ollama").unwrap();
        assert!(p.enabled && p.local);
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'ollama'", []).unwrap();
        let before = server.requests.lock().unwrap().len();
        assert_eq!(test("ollama").await.detail, "This connection is disabled.");
        let refused = complete(&state, "ollama", Some("a:1".into()), None, vec![ChatMessage { role: "user".into(), content: "hi".into() }])
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("disabled"), "{refused}");
        assert!(usable_provider(&state, "ollama").is_err());
        assert_eq!(server.requests.lock().unwrap().len(), before);
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    /// Talks to a real local HTTP server through the OpenAI-compatible path: an error response,
    /// a good response, then a dead server.
    #[tokio::test]
    async fn surfaces_provider_http_failures_and_recovers() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for n in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 16_384];
                let mut read = 0;
                // Read the whole request: closing with unread bytes would reset the connection
                // and could discard the response before the client reads it.
                loop {
                    read += socket.read(&mut buf[read..]).await.unwrap();
                    let text = String::from_utf8_lossy(&buf[..read]).to_lowercase();
                    let Some(head_end) = text.find("\r\n\r\n") else { continue };
                    let length: usize = text[..head_end]
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if read >= head_end + 4 + length {
                        break;
                    }
                }
                let (status, body) = if n == 0 {
                    ("500 Internal Server Error", r#"{"error":{"message":"model overloaded"}}"#)
                } else {
                    ("200 OK", r#"{"choices":[{"message":{"content":"pong"}}],"usage":{"prompt_tokens":7,"completion_tokens":2}}"#)
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        let state = AppState::for_tests();
        state.db.conn().execute(
            "INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at) VALUES ('local', 'Local', 'openai', ?1, 'test-model', 0, '')",
            [format!("http://127.0.0.1:{port}/v1")],
        ).unwrap();
        let ask = || complete(&state, "local", None, None, vec![ChatMessage { role: "user".into(), content: "ping".into() }]);

        let err = ask().await.unwrap_err().to_string();
        assert!(err.contains("500") && err.contains("model overloaded"), "{err}");
        let ok = ask().await.unwrap();
        assert_eq!((ok.content.as_str(), ok.model.as_str(), ok.input_tokens, ok.output_tokens), ("pong", "test-model", Some(7), Some(2)));
        server.await.unwrap();
        // A port nothing listens on. (The server's own port may already belong to another test.)
        state.db.conn().execute("UPDATE providers SET base_url = 'http://127.0.0.1:9/v1' WHERE id = 'local'", []).unwrap();
        let dead = ask().await.unwrap_err().to_string();
        assert!(dead.contains("Could not connect"), "{dead}");
        // Usage is recorded for the successful call only.
        let rows: i64 = state.db.conn().query_row("SELECT COUNT(*) FROM ai_usage", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }
}
