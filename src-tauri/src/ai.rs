//! AI workspace: provider configuration, secure credential storage, model discovery and chat.
//! Everything here is optional; the rest of NEXUS works with no provider configured.

use crate::error::{AppError, Result};
use crate::{activity, db, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::State;

const KEYRING_SERVICE: &str = "dev.nexus.workspace";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// "openai" (any OpenAI-compatible API), "anthropic" or "ollama".
    pub kind: String,
    pub base_url: String,
    pub default_model: Option<String>,
    pub has_key: bool,
}

#[derive(Deserialize)]
pub struct ProviderInput {
    id: Option<String>,
    name: String,
    kind: String,
    base_url: String,
    default_model: Option<String>,
    /// None leaves the stored key untouched; an empty string removes it.
    api_key: Option<String>,
}

fn keyring_entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, id).map_err(|e| AppError::Msg(format!("Secure storage unavailable: {e}")))
}

fn read_key(id: &str) -> Result<Option<String>> {
    match keyring_entry(id)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Msg(format!("Could not read the API key from secure storage: {e}"))),
    }
}

fn row_to_provider(r: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        base_url: r.get(3)?,
        default_model: r.get(4)?,
        has_key: r.get::<_, i64>(5)? != 0,
    })
}

const COLS: &str = "id, name, kind, base_url, default_model, has_key";

fn get_provider(state: &AppState, id: &str) -> Result<Provider> {
    state
        .db
        .conn()
        .query_row(&format!("SELECT {COLS} FROM providers WHERE id = ?1"), [id], row_to_provider)
        .optional()?
        .ok_or_else(|| "Provider not found".into())
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<Provider>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM providers ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_provider)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn save_provider(state: State<'_, AppState>, input: ProviderInput) -> Result<Provider> {
    if !matches!(input.kind.as_str(), "openai" | "anthropic" | "ollama") {
        return Err("Unknown provider type".into());
    }
    let name = input.name.trim();
    let base_url = input.base_url.trim().trim_end_matches('/');
    if name.is_empty() {
        return Err("Name is required".into());
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err("Base URL must start with http:// or https://".into());
    }
    let id = input.id.unwrap_or_else(db::new_id);
    let model = input.default_model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());

    // Credentials go to the OS keychain only; SQLite stores just a "has key" flag.
    let mut has_key = read_key(&id).unwrap_or(None).is_some();
    if let Some(key) = input.api_key {
        let key = key.trim();
        if key.is_empty() {
            let _ = keyring_entry(&id)?.delete_credential();
            has_key = false;
        } else {
            keyring_entry(&id)?
                .set_password(key)
                .map_err(|e| AppError::Msg(format!("Could not store the API key securely: {e}")))?;
            has_key = true;
        }
    }
    let conn = state.db.conn();
    conn.execute(
        "INSERT INTO providers (id, name, kind, base_url, default_model, has_key, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET name = ?2, kind = ?3, base_url = ?4, default_model = ?5, has_key = ?6",
        params![id, name, input.kind, base_url, model, has_key as i64, db::now()],
    )?;
    activity::log(&conn, "ai", &format!("Saved AI provider {name}"), None);
    Ok(conn.query_row(&format!("SELECT {COLS} FROM providers WHERE id = ?1"), [&id], row_to_provider)?)
}

#[tauri::command]
pub fn delete_provider(state: State<'_, AppState>, id: String) -> Result<()> {
    if let Ok(entry) = keyring_entry(&id) {
        let _ = entry.delete_credential();
    }
    state.db.conn().execute("DELETE FROM providers WHERE id = ?1", [&id])?;
    Ok(())
}

/// Attach the provider's authentication headers to a request.
fn authed(provider: &Provider, req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
    let key = if provider.has_key { read_key(&provider.id)? } else { None };
    Ok(match (provider.kind.as_str(), key) {
        ("anthropic", Some(k)) => req.header("x-api-key", k).header("anthropic-version", "2023-06-01"),
        ("anthropic", None) => return Err("This provider needs an API key".into()),
        (_, Some(k)) => req.bearer_auth(k),
        (_, None) => req,
    })
}

async fn send_json(req: reqwest::RequestBuilder) -> Result<Value> {
    let resp = req.timeout(REQUEST_TIMEOUT).send().await.map_err(|e| {
        if e.is_connect() {
            AppError::Msg("Could not connect. Check the base URL and that the server is running.".into())
        } else if e.is_timeout() {
            AppError::Msg("The request timed out".into())
        } else {
            AppError::Http(e)
        }
    })?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                let e = v.get("error")?;
                e.get("message").and_then(|m| m.as_str()).or(e.as_str()).map(String::from)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(AppError::Msg(format!("Provider returned {status}: {detail}")));
    }
    Ok(serde_json::from_str(&text)?)
}

#[derive(Serialize)]
pub struct ModelInfo {
    id: String,
    size_bytes: Option<u64>,
    family: Option<String>,
    parameter_size: Option<String>,
    quantization: Option<String>,
    modified_at: Option<String>,
}

fn parse_ollama_tags(v: &Value) -> Vec<ModelInfo> {
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

fn parse_model_list(v: &Value) -> Vec<ModelInfo> {
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

async fn fetch_models(state: &AppState, p: &Provider) -> Result<Vec<ModelInfo>> {
    match p.kind.as_str() {
        "ollama" => {
            let v = send_json(authed(p, state.http.get(format!("{}/api/tags", p.base_url)))?).await?;
            Ok(parse_ollama_tags(&v))
        }
        "anthropic" => {
            let v = send_json(authed(p, state.http.get(format!("{}/v1/models?limit=100", p.base_url)))?).await?;
            Ok(parse_model_list(&v))
        }
        _ => {
            let v = send_json(authed(p, state.http.get(format!("{}/models", p.base_url)))?).await?;
            Ok(parse_model_list(&v))
        }
    }
}

/// Lists models for a provider; doubles as the connection test.
#[tauri::command]
pub async fn list_models(state: State<'_, AppState>, provider_id: String) -> Result<Vec<ModelInfo>> {
    let p = get_provider(&state, &provider_id)?;
    fetch_models(&state, &p).await
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

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
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
    let p = get_provider(state, provider_id)?;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .or_else(|| p.default_model.clone())
        .ok_or("Choose a model first")?;
    let system = system.filter(|s| !s.trim().is_empty());
    let started = Instant::now();
    let u = |v: &Value, path: &[&str]| path.iter().try_fold(v, |acc, k| acc.get(k)).and_then(|x| x.as_u64());

    let (content, input_tokens, output_tokens) = match p.kind.as_str() {
        "anthropic" => {
            let mut body = json!({ "model": model, "max_tokens": 8192, "messages": messages });
            if let Some(s) = &system {
                body["system"] = json!(s);
            }
            let v = send_json(authed(&p, state.http.post(format!("{}/v1/messages", p.base_url)))?.json(&body)).await?;
            let text = v
                .get("content")
                .and_then(|c| c.as_array())
                .map(|blocks| {
                    blocks.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("")
                })
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
                let v = send_json(authed(&p, state.http.post(format!("{}/api/chat", p.base_url)))?.json(&body)).await?;
                let text = v.pointer("/message/content").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                (text, u(&v, &["prompt_eval_count"]), u(&v, &["eval_count"]))
            } else {
                let body = json!({ "model": model, "messages": msgs });
                let v = send_json(authed(&p, state.http.post(format!("{}/chat/completions", p.base_url)))?.json(&body))
                    .await?;
                let text =
                    v.pointer("/choices/0/message/content").and_then(|t| t.as_str()).unwrap_or_default().to_string();
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
        Ok(UsageRow {
            provider: r.get(0)?,
            model: r.get(1)?,
            requests: r.get(2)?,
            input_tokens: r.get(3)?,
            output_tokens: r.get(4)?,
        })
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
}
