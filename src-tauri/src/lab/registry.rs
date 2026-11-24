//! The model library: every model a connected runtime reports, plus model files in the storage
//! folder. Each record keeps only what the runtime or the file itself stated; a field nobody
//! reported stays empty and is shown as unknown. Records are cached in SQLite so the library
//! still opens when a runtime is offline.

use super::{gguf, storage};
use crate::ai::{self, Provider, PROBE_TIMEOUT};
use crate::error::{AppError, Result};
use crate::{activity, db, AppState};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::State;

const MAX_LICENSE_TEXT: usize = 20_000;

pub fn provider_key(provider_id: &str, model_id: &str) -> String {
    format!("{provider_id}\n{model_id}")
}

pub fn file_key(name: &str) -> String {
    format!("file\n{name}")
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct ModelMeta {
    pub display_name: Option<String>,
    pub family: Option<String>,
    pub architecture: Option<String>,
    pub parameter_count: Option<u64>,
    /// Size class as the runtime words it, for example "27.8B".
    pub parameter_label: Option<String>,
    pub quantization: Option<String>,
    pub context_length: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub capabilities: Vec<String>,
    pub format: Option<String>,
    pub size_bytes: Option<u64>,
    /// Short licence name or identifier.
    pub license: Option<String>,
    pub license_text: Option<String>,
    pub source_url: Option<String>,
    pub modified_at: Option<String>,
    pub digest: Option<String>,
    pub runner: Option<String>,
    /// The runtime's own default generation settings, as it prints them.
    pub runtime_defaults: Option<String>,
    pub path: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelRecord {
    pub key: String,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    /// The connection's kind, or "file" for a model file no runtime is serving.
    pub kind: String,
    pub model_id: String,
    /// True when using the model keeps requests on this machine.
    pub local: bool,
    /// "installed" (in a local runtime), "remote" (served elsewhere), "file" (on disk),
    /// "disabled" (its connection is switched off) or "missing" (a favourite its runtime no
    /// longer lists).
    pub status: String,
    pub favorite: bool,
    /// A file NEXUS downloaded.
    pub managed: bool,
    /// NEXUS can remove it on request.
    pub removable: bool,
    pub meta: ModelMeta,
    /// Where the details came from.
    pub meta_source: String,
    pub compatibility: Vec<String>,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Serialize, Debug)]
pub struct SourceStatus {
    pub provider_id: Option<String>,
    pub name: String,
    pub kind: String,
    /// False when the source was skipped (disabled) or could not be reached.
    pub ok: bool,
    pub error: Option<String>,
    pub models: usize,
}

#[derive(Serialize, Debug)]
pub struct Library {
    pub models: Vec<ModelRecord>,
    /// One entry per source checked by the last refresh; empty for a cached listing.
    pub sources: Vec<SourceStatus>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Details of an Ollama model from its `/api/tags` entry and its `/api/show` reply.
pub fn meta_from_ollama(tag: &Value, show: &Value) -> ModelMeta {
    let null = Value::Null;
    let details = show.get("details").filter(|d| d.is_object()).or(tag.get("details")).unwrap_or(&null);
    let info = show.get("model_info").unwrap_or(&null);
    let architecture = text(info, "general.architecture");
    let context_length = info.as_object().and_then(|o| {
        let own = architecture.as_ref().map(|a| format!("{a}.context_length"));
        o.iter()
            .find(|(k, _)| Some(*k) == own.as_ref())
            .or_else(|| o.iter().find(|(k, _)| k.ends_with(".context_length")))
            .and_then(|(_, v)| v.as_u64())
    });
    let license_text = text(show, "license");
    let mut notes = vec![];
    if let Some(requires) = text(show, "requires") {
        notes.push(format!("Needs Ollama {requires} or newer"));
    }
    if let Some(parent) = text(details, "parent_model") {
        notes.push(format!("Based on {parent}"));
    }
    ModelMeta {
        family: text(details, "family"),
        architecture,
        parameter_count: info.get("general.parameter_count").and_then(|v| v.as_u64()),
        parameter_label: text(details, "parameter_size"),
        quantization: text(details, "quantization_level"),
        context_length,
        capabilities: show
            .get("capabilities")
            .and_then(|c| c.as_array())
            .map(|list| list.iter().filter_map(|c| c.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        format: text(details, "format"),
        size_bytes: tag.get("size").and_then(|v| v.as_u64()),
        license: license_text.as_ref().and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(80).collect())),
        license_text: license_text.map(|t| t.chars().take(MAX_LICENSE_TEXT).collect()),
        modified_at: text(tag, "modified_at").or_else(|| text(show, "modified_at")),
        digest: text(tag, "digest"),
        runner: text(details, "runner"),
        runtime_defaults: text(show, "parameters"),
        notes,
        ..Default::default()
    }
}

/// Details of the model a llama.cpp server has loaded, from `/v1/models` and `/props`.
pub fn meta_from_llamacpp(model: &Value, props: &Value) -> ModelMeta {
    let null = Value::Null;
    let meta = model.get("meta").unwrap_or(&null);
    let mut capabilities = vec![];
    if let Some(inputs) = model.pointer("/architecture/input_modalities").and_then(|m| m.as_array()) {
        for modality in inputs.iter().filter_map(|m| m.as_str()) {
            capabilities.push(if modality == "image" { "vision".to_string() } else { modality.to_string() });
        }
    }
    let mut notes = vec![];
    if let Some(n_ctx) = props.pointer("/default_generation_settings/n_ctx").and_then(|v| v.as_u64()) {
        notes.push(format!("The server is running with a {n_ctx}-token context window"));
    }
    ModelMeta {
        parameter_count: meta.get("n_params").and_then(|v| v.as_u64()),
        context_length: meta.get("n_ctx_train").and_then(|v| v.as_u64()),
        size_bytes: meta.get("size").and_then(|v| v.as_u64()),
        capabilities,
        format: Some("gguf".into()),
        path: text(props, "model_path"),
        notes,
        ..Default::default()
    }
}

/// Details of a Claude model from the Models API.
pub fn meta_from_anthropic(model: &Value) -> ModelMeta {
    let supported = |path: &str| model.pointer(path).and_then(|v| v.as_bool()).unwrap_or(false);
    let mut capabilities = vec![];
    for (path, name) in [
        ("/capabilities/image_input/supported", "vision"),
        ("/capabilities/thinking/supported", "thinking"),
        ("/capabilities/structured_outputs/supported", "structured outputs"),
    ] {
        if supported(path) {
            capabilities.push(name.to_string());
        }
    }
    ModelMeta {
        display_name: text(model, "display_name"),
        context_length: model.get("max_input_tokens").and_then(|v| v.as_u64()),
        max_output_tokens: model.get("max_tokens").and_then(|v| v.as_u64()),
        capabilities,
        modified_at: text(model, "created_at"),
        ..Default::default()
    }
}

/// An OpenAI-style listing says very little: a name and sometimes an owner.
pub fn meta_from_listing(model: &Value) -> ModelMeta {
    ModelMeta { notes: text(model, "owned_by").map(|o| vec![format!("Listed as owned by {o}")]).unwrap_or_default(), ..Default::default() }
}

pub fn meta_from_gguf(info: &gguf::GgufInfo, size_bytes: u64, path: &str) -> ModelMeta {
    let mut notes = vec![];
    if info.parameter_source == Some("tensor shapes") {
        notes.push("Parameter count was added up from the tensor shapes in the file".to_string());
    }
    if let Some(blocks) = info.block_count {
        notes.push(format!("{blocks} layers"));
    }
    ModelMeta {
        display_name: info.name.clone(),
        family: info.basename.clone(),
        architecture: info.architecture.clone(),
        parameter_count: info.parameter_count,
        parameter_label: info.size_label.clone(),
        quantization: info.quantization.clone(),
        context_length: info.context_length,
        capabilities: if info.has_chat_template { vec!["chat template".into()] } else { vec![] },
        format: Some(format!("gguf v{}", info.version)),
        size_bytes: Some(size_bytes),
        license: info.license.clone(),
        source_url: info.url.clone().filter(|u| u.starts_with("https://") || u.starts_with("http://")),
        path: Some(path.to_string()),
        notes,
        ..Default::default()
    }
}

type Found = (String, ModelMeta, &'static str);

/// Ask one provider what it serves.
async fn fetch_provider_models(state: &AppState, p: &Provider) -> Result<Vec<Found>> {
    let get = |url: String| ai::authed(p, state.http.get(url));
    match p.kind.as_str() {
        "ollama" => {
            let tags = ai::send_json_within(get(format!("{}/api/tags", p.base_url))?, PROBE_TIMEOUT).await?;
            let mut out = vec![];
            for tag in tags.get("models").and_then(|m| m.as_array()).into_iter().flatten() {
                let Some(id) = text(tag, "name") else { continue };
                let body = json!({ "model": id });
                let request = ai::authed(p, state.http.post(format!("{}/api/show", p.base_url)).json(&body))?;
                // A model whose details cannot be read is still listed, with what the listing says.
                match ai::send_json_within(request, PROBE_TIMEOUT).await {
                    Ok(show) => out.push((id, meta_from_ollama(tag, &show), "Reported by Ollama (/api/show)")),
                    Err(_) => {
                        out.push((id, meta_from_ollama(tag, &Value::Null), "Reported by Ollama (/api/tags); details were not available"))
                    }
                }
            }
            Ok(out)
        }
        "llamacpp" => {
            let list = ai::send_json_within(get(format!("{}/v1/models", p.base_url))?, PROBE_TIMEOUT).await?;
            let props = match get(format!("{}/props", p.base_url)) {
                Ok(request) => ai::send_json_within(request, PROBE_TIMEOUT).await.unwrap_or(Value::Null),
                Err(_) => Value::Null,
            };
            Ok(list
                .get("data")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some((text(m, "id")?, meta_from_llamacpp(m, &props), "Reported by the llama.cpp server (/v1/models, /props)"))
                })
                .collect())
        }
        "anthropic" => {
            let list = ai::send_json_within(get(format!("{}/v1/models?limit=100", p.base_url))?, PROBE_TIMEOUT).await?;
            Ok(list
                .get("data")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| Some((text(m, "id")?, meta_from_anthropic(m), "Reported by the Anthropic Models API")))
                .collect())
        }
        _ => {
            let list = ai::send_json_within(get(format!("{}/models", ai::openai_base(p)))?, PROBE_TIMEOUT).await?;
            Ok(list
                .get("data")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| Some((text(m, "id")?, meta_from_listing(m), "Listed by the endpoint, which reports no specifications")))
                .collect())
        }
    }
}

fn upsert(state: &AppState, key: &str, provider_id: Option<&str>, model_id: &str, meta: &ModelMeta, source: &str) -> Result<()> {
    let now = db::now();
    state.db.conn().execute(
        "INSERT INTO lab_models (key, provider_id, model_id, meta, meta_source, available, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
         ON CONFLICT(key) DO UPDATE SET meta = excluded.meta, meta_source = excluded.meta_source, available = 1, last_seen = excluded.last_seen",
        params![key, provider_id, model_id, serde_json::to_string(meta)?, source, now],
    )?;
    Ok(())
}

/// Forget records of one source that it no longer reports. Favourites are kept and marked missing.
fn prune(state: &AppState, provider_id: Option<&str>, keep: &HashSet<String>) -> Result<()> {
    let conn = state.db.conn();
    let keys: Vec<String> = match provider_id {
        Some(id) => conn
            .prepare("SELECT key FROM lab_models WHERE provider_id = ?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
        None => conn
            .prepare("SELECT key FROM lab_models WHERE provider_id IS NULL")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
    };
    for key in keys.into_iter().filter(|k| !keep.contains(k)) {
        conn.execute("DELETE FROM lab_models WHERE key = ?1 AND favorite = 0", [&key])?;
        conn.execute("UPDATE lab_models SET available = 0 WHERE key = ?1", [&key])?;
    }
    Ok(())
}

/// Read the model files in the storage folder into the library.
fn scan_files(state: &AppState) -> Result<SourceStatus> {
    let dir = storage::dir(state)?;
    let info = storage::info(state)?;
    let mut keep = HashSet::new();
    let mut unreadable = 0;
    for file in info.files.iter().filter(|f| !f.partial) {
        let path = dir.join(&file.name);
        let key = file_key(&file.name);
        let (meta, source) = match gguf::read(&path) {
            Ok(header) => (meta_from_gguf(&header, file.size_bytes, &path.to_string_lossy()), "Read from the GGUF file header"),
            Err(e) => {
                unreadable += 1;
                let meta = ModelMeta {
                    size_bytes: Some(file.size_bytes),
                    path: Some(path.to_string_lossy().into_owned()),
                    notes: vec![format!("The file header could not be read: {e}")],
                    ..Default::default()
                };
                (meta, "File on disk; its header could not be read")
            }
        };
        upsert(state, &key, None, &file.name, &meta, source)?;
        keep.insert(key);
    }
    prune(state, None, &keep)?;
    Ok(SourceStatus {
        provider_id: None,
        name: "Model folder".into(),
        kind: "file".into(),
        ok: true,
        error: (unreadable > 0).then(|| format!("{unreadable} file(s) could not be read as GGUF")),
        models: keep.len(),
    })
}

fn compatibility(kind: &str, provider_name: Option<&str>, meta: &ModelMeta) -> Vec<String> {
    match kind {
        "ollama" => vec![match &meta.runner {
            Some(runner) => format!("Ollama ({runner} runner)"),
            None => "Ollama".to_string(),
        }],
        "llamacpp" => vec!["llama.cpp server".into()],
        "anthropic" => vec!["Anthropic API".into()],
        "file" => vec!["llama.cpp server (loads GGUF files directly)".into(), "Ollama (after importing the file with a Modelfile)".into()],
        _ => vec![format!("OpenAI-compatible endpoint{}", provider_name.map(|n| format!(": {n}")).unwrap_or_default())],
    }
}

/// The cached library, without contacting anything.
pub fn list(state: &AppState) -> Result<Vec<ModelRecord>> {
    let providers = ai::all_providers(state)?;
    let dir = storage::dir(state)?;
    #[allow(clippy::type_complexity)] // rows of columns, destructured right below
    let rows: Vec<(String, Option<String>, String, String, String, bool, bool, String, String)> = {
        let conn = state.db.conn();
        let mut stmt = conn
            .prepare("SELECT key, provider_id, model_id, meta, meta_source, favorite, available, first_seen, last_seen FROM lab_models")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get::<_, i64>(5)? != 0,
                r.get::<_, i64>(6)? != 0,
                r.get(7)?,
                r.get(8)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut out = vec![];
    for (key, provider_id, model_id, meta, meta_source, favorite, available, first_seen, last_seen) in rows {
        let meta: ModelMeta = serde_json::from_str(&meta).unwrap_or_default();
        let provider = provider_id.as_ref().and_then(|id| providers.iter().find(|p| &p.id == id));
        // A record whose connection was deleted has nothing to point at.
        if provider_id.is_some() && provider.is_none() {
            continue;
        }
        let kind = provider.map(|p| p.kind.clone()).unwrap_or_else(|| "file".into());
        let local = provider.map(|p| p.local).unwrap_or(true);
        let managed = provider.is_none() && storage::is_managed(state, &dir.join(&model_id));
        let status = match (provider, available) {
            (_, false) => "missing",
            (None, true) => "file",
            // A disabled connection is never contacted, so its models cannot be used.
            (Some(p), true) if !p.enabled => "disabled",
            (Some(p), true) if p.local => "installed",
            (Some(_), true) => "remote",
        };
        out.push(ModelRecord {
            compatibility: compatibility(&kind, provider.map(|p| p.name.as_str()), &meta),
            removable: available && (managed || (kind == "ollama" && local && provider.is_some_and(|p| p.enabled))),
            provider_name: provider.map(|p| p.name.clone()),
            key,
            provider_id,
            kind,
            model_id,
            local,
            status: status.into(),
            favorite,
            managed,
            meta,
            meta_source,
            first_seen,
            last_seen,
        });
    }
    out.sort_by_key(|a| (!a.favorite, a.model_id.to_lowercase()));
    Ok(out)
}

/// Ask every enabled connection and the model folder what they have, and update the library.
pub async fn refresh(state: &AppState) -> Result<Library> {
    let providers = ai::all_providers(state)?;
    // Records of deleted connections go first.
    {
        let ids: Vec<String> = providers.iter().map(|p| p.id.clone()).collect();
        let conn = state.db.conn();
        let stale: Vec<String> = conn
            .prepare("SELECT DISTINCT provider_id FROM lab_models WHERE provider_id IS NOT NULL")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in stale.into_iter().filter(|id| !ids.contains(id)) {
            conn.execute("DELETE FROM lab_models WHERE provider_id = ?1", [&id])?;
        }
    }
    let mut sources = vec![];
    for p in &providers {
        let mut status =
            SourceStatus { provider_id: Some(p.id.clone()), name: p.name.clone(), kind: p.kind.clone(), ok: false, error: None, models: 0 };
        if !p.enabled {
            status.error = Some("Disabled".into());
            sources.push(status);
            continue;
        }
        match fetch_provider_models(state, p).await {
            Ok(found) => {
                let mut keep = HashSet::new();
                for (model_id, meta, source) in &found {
                    let key = provider_key(&p.id, model_id);
                    upsert(state, &key, Some(&p.id), model_id, meta, source)?;
                    keep.insert(key);
                }
                prune(state, Some(&p.id), &keep)?;
                status.ok = true;
                status.models = found.len();
            }
            // An unreachable runtime keeps its last known models in the library.
            Err(e) => status.error = Some(e.to_string()),
        }
        sources.push(status);
    }
    sources.push(scan_files(state)?);
    Ok(Library { models: list(state)?, sources })
}

fn record(state: &AppState, key: &str) -> Result<ModelRecord> {
    list(state)?.into_iter().find(|m| m.key == key).ok_or_else(|| AppError::Msg("That model is no longer in the library".into()))
}

/// Remove a model: a file NEXUS downloaded, or a model in a local Ollama.
pub async fn remove(state: &AppState, key: &str) -> Result<()> {
    let model = record(state, key)?;
    if !model.removable {
        return Err("NEXUS cannot remove this model. It only removes files it downloaded and models in a local Ollama.".into());
    }
    if model.kind == "file" {
        return storage::delete_file(state, &model.model_id);
    }
    let provider = ai::usable_provider(state, model.provider_id.as_deref().unwrap_or(""))?;
    let body = json!({ "model": model.model_id });
    let request = ai::authed(&provider, state.http.delete(format!("{}/api/delete", provider.base_url)).json(&body))?;
    let ai::Authed { req, redactor } = request;
    let resp = req.timeout(PROBE_TIMEOUT).send().await.map_err(|e| ai::transport_error(e, &redactor))?;
    let status = resp.status();
    if !status.is_success() {
        let body = ai::read_body_prefix(resp, ai::MAX_ERROR_BYTES).await;
        return Err(ai::error_detail(status, &body, &redactor));
    }
    let conn = state.db.conn();
    conn.execute("DELETE FROM lab_models WHERE key = ?1", [key])?;
    activity::log(&conn, "models", &format!("Removed {} from {}", model.model_id, provider.name), None);
    Ok(())
}

#[tauri::command]
pub fn lab_models_list(state: State<'_, AppState>) -> Result<Library> {
    Ok(Library { models: list(&state)?, sources: vec![] })
}

#[tauri::command]
pub async fn lab_models_refresh(state: State<'_, AppState>) -> Result<Library> {
    refresh(&state).await
}

#[tauri::command]
pub fn lab_model_favorite(state: State<'_, AppState>, key: String, favorite: bool) -> Result<()> {
    let conn = state.db.conn();
    if conn.execute("UPDATE lab_models SET favorite = ?2 WHERE key = ?1", params![key, favorite as i64])? == 0 {
        return Err("That model is no longer in the library".into());
    }
    // A missing model is only kept while it is a favourite.
    conn.execute("DELETE FROM lab_models WHERE key = ?1 AND favorite = 0 AND available = 0", [&key])?;
    Ok(())
}

#[tauri::command]
pub async fn lab_model_remove(state: State<'_, AppState>, key: String) -> Result<()> {
    remove(&state, &key).await
}

/// Model details for a provider's model, if the library has seen it.
pub fn lookup(state: &AppState, provider_id: &str, model_id: &str) -> Option<ModelMeta> {
    let raw: Option<String> = state
        .db
        .conn()
        .query_row("SELECT meta FROM lab_models WHERE key = ?1", [provider_key(provider_id, model_id)], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    raw.and_then(|m| serde_json::from_str(&m).ok())
}

#[cfg(test)]
mod tests {
    use super::super::testserver::{serve, Req, Resp};
    use super::*;
    use std::fs;

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

    /// The reply shapes below were captured from a real Ollama 0.40 (trimmed).
    fn ollama(req: &Req) -> Resp {
        match req.path.as_str() {
            "/api/tags" => Resp::json(json!({"models": [
                {"name": "qwen3.8:27b", "model": "qwen3.8:27b", "modified_at": "2026-11-13T21:19:27+05:30", "size": 18174721847u64,
                 "digest": "47951e4a6661", "details": {"parent_model": "", "format": "safetensors", "family": "", "families": null, "parameter_size": "", "quantization_level": "nvfp4"}},
                {"name": "broken:latest", "size": 10, "details": {"format": "gguf", "family": "llama", "parameter_size": "1B", "quantization_level": "Q4_0"}}
            ]})),
            "/api/show" if req.json()["model"] == "qwen3.8:27b" => Resp::json(json!({
                "license": "\n\n   Apache License\n   Version 2.0, January 2004\n",
                "parameters": "temperature 1\ntop_k 20",
                "details": {"parent_model": "", "format": "safetensors", "family": "qwen3_5", "families": null, "parameter_size": "27.8B", "quantization_level": "nvfp4", "runner": "mlx"},
                "model_info": {"general.architecture": "qwen3_5", "general.parameter_count": 27781081984u64, "qwen3_5.block_count": 64, "qwen3_5.context_length": 262144, "qwen3_5.embedding_length": 5120},
                "capabilities": ["completion", "vision", "tools", "thinking"],
                "requires": "0.32.12"
            })),
            "/api/show" => Resp::error(500, json!({"error": "model is corrupt"})),
            _ => Resp::error(404, json!({"error": "not found"})),
        }
    }

    #[tokio::test]
    async fn library_merges_runtimes_and_files_and_keeps_unknown_fields_empty() {
        let ollama_server = serve(ollama).await;
        let llama = serve(|req| match req.path.as_str() {
            "/v1/models" => Resp::json(json!({"object": "list", "data": [{"id": "models/tiny.gguf", "object": "model", "owned_by": "llamacpp",
                "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
                "meta": {"vocab_type": 2, "n_vocab": 32000, "n_ctx_train": 131072, "n_embd": 4096, "n_params": 8030261312u64, "size": 4912898304u64}}]})),
            "/props" => Resp::json(json!({"default_generation_settings": {"n_ctx": 4096}, "model_path": "/models/tiny.gguf"})),
            _ => Resp::error(404, json!({"error": {"message": "nope"}})),
        }).await;
        let generic = serve(|req| match req.path.as_str() {
            "/v1/models" => Resp::json(json!({"object": "list", "data": [{"id": "served-model", "owned_by": "vllm"}]})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let claude = serve(|req| match req.path.as_str() {
            "/v1/models?limit=100" => Resp::json(json!({"data": [{"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "created_at": "2026-08-01T00:00:00Z",
                "max_input_tokens": 1000000, "max_tokens": 128000,
                "capabilities": {"image_input": {"supported": true}, "structured_outputs": {"supported": true}, "thinking": {"supported": true}, "batch": {"supported": false}}}]})),
            _ => Resp::error(404, json!({})),
        }).await;

        let state = AppState::for_tests();
        add_provider(&state, "a-ollama", "ollama", &ollama_server.url);
        add_provider(&state, "b-llama", "llamacpp", &llama.url);
        add_provider(&state, "c-generic", "openai", &format!("{}/v1", generic.url));
        add_provider(&state, "d-claude", "anthropic", &claude.url);
        add_provider(&state, "e-off", "ollama", "http://127.0.0.1:9");
        add_provider(&state, "f-down", "ollama", "http://127.0.0.1:9");
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'e-off'", []).unwrap();
        // Anthropic needs a key; without one the source reports an error instead of a guess.
        let dir = storage::dir(&state).unwrap();
        fs::write(dir.join("tiny.gguf"), gguf::testfile::sample()).unwrap();
        fs::write(dir.join("garbage.gguf"), b"not a model").unwrap();

        let lib = refresh(&state).await.unwrap();
        let source = |id: &str| lib.sources.iter().find(|s| s.provider_id.as_deref() == Some(id)).unwrap();
        assert!(source("a-ollama").ok && source("a-ollama").models == 2);
        assert!(source("b-llama").ok && source("c-generic").ok);
        assert!(!source("d-claude").ok && source("d-claude").error.as_deref().unwrap().contains("API key"));
        assert_eq!(source("e-off").error.as_deref(), Some("Disabled"));
        assert!(source("f-down").error.as_deref().unwrap().contains("Could not connect"));
        let folder = lib.sources.iter().find(|s| s.kind == "file").unwrap();
        assert_eq!(folder.models, 2);
        assert!(folder.error.as_deref().unwrap().contains("1 file"));

        let model = |id: &str| lib.models.iter().find(|m| m.model_id == id).unwrap_or_else(|| panic!("{id} missing"));
        let q = model("qwen3.8:27b");
        assert_eq!((q.kind.as_str(), q.status.as_str(), q.local, q.removable), ("ollama", "installed", true, true));
        assert_eq!(q.meta.parameter_count, Some(27781081984));
        assert_eq!(q.meta.parameter_label.as_deref(), Some("27.8B"));
        assert_eq!(q.meta.context_length, Some(262144));
        assert_eq!(q.meta.quantization.as_deref(), Some("nvfp4"));
        assert_eq!(q.meta.family.as_deref(), Some("qwen3_5"), "details from /api/show win over the emptier listing");
        assert_eq!(q.meta.license.as_deref(), Some("Apache License"));
        assert_eq!(q.meta.capabilities, ["completion", "vision", "tools", "thinking"]);
        assert_eq!(q.meta.size_bytes, Some(18174721847));
        assert_eq!(q.compatibility, ["Ollama (mlx runner)"]);
        assert!(q.meta.notes.iter().any(|n| n.contains("0.32.12")));

        let broken = model("broken:latest");
        assert!(broken.meta_source.contains("details were not available"));
        assert_eq!((broken.meta.quantization.as_deref(), broken.meta.context_length), (Some("Q4_0"), None));

        let served = model("models/tiny.gguf");
        assert_eq!((served.meta.parameter_count, served.meta.context_length), (Some(8030261312), Some(131072)));
        assert!(served.meta.capabilities.contains(&"vision".to_string()));
        assert!(served.meta.notes[0].contains("4096-token"));
        assert!(!served.removable);

        let listed = model("served-model");
        assert_eq!(listed.meta.parameter_count, None);
        assert_eq!(listed.meta.context_length, None);
        assert_eq!(listed.meta.quantization, None);
        assert!(listed.meta_source.contains("no specifications"));

        let file = model("tiny.gguf");
        assert_eq!((file.kind.as_str(), file.status.as_str(), file.managed, file.removable), ("file", "file", false, false));
        assert_eq!(file.meta.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(file.meta.context_length, Some(2048));
        assert_eq!(file.meta.parameter_count, Some(64));
        assert!(model("garbage.gguf").meta.notes[0].contains("could not be read"));

        // Switching a connection off keeps its models listed, marked as unusable.
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'a-ollama'", []).unwrap();
        let after = refresh(&state).await.unwrap();
        let off = after.models.iter().find(|m| m.model_id == "qwen3.8:27b").unwrap();
        assert_eq!((off.status.as_str(), off.removable), ("disabled", false));
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn offline_runtimes_keep_their_models_and_removed_models_go_unless_favourited() {
        let models = std::sync::Arc::new(std::sync::Mutex::new(vec!["keep:1", "fav:1", "drop:1"]));
        let served = models.clone();
        let server = serve(move |req| match req.path.as_str() {
            "/api/tags" => {
                Resp::json(json!({"models": served.lock().unwrap().iter().map(|m| json!({"name": m, "size": 5})).collect::<Vec<_>>()}))
            }
            "/api/show" => Resp::json(json!({"details": {"family": "x"}})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "p", "ollama", &server.url);
        assert_eq!(refresh(&state).await.unwrap().models.len(), 3);
        lab_fav(&state, "p", "fav:1");

        // The runtime goes away: nothing is lost.
        state.db.conn().execute("UPDATE providers SET base_url = 'http://127.0.0.1:9' WHERE id = 'p'", []).unwrap();
        let offline = refresh(&state).await.unwrap();
        assert!(!offline.sources[0].ok);
        assert_eq!(offline.models.len(), 3);
        assert_eq!(offline.models[0].model_id, "fav:1", "favourites sort first");

        // It comes back with two models uninstalled.
        state.db.conn().execute("UPDATE providers SET base_url = ?1 WHERE id = 'p'", [&server.url]).unwrap();
        *models.lock().unwrap() = vec!["keep:1"];
        let back = refresh(&state).await.unwrap();
        let ids: Vec<(&str, &str)> = back.models.iter().map(|m| (m.model_id.as_str(), m.status.as_str())).collect();
        assert_eq!(ids, [("fav:1", "missing"), ("keep:1", "installed")]);
        assert!(!back.models[0].removable);

        // Deleting the connection removes its records, favourites included.
        state.db.conn().execute("DELETE FROM providers WHERE id = 'p'", []).unwrap();
        assert!(refresh(&state).await.unwrap().models.is_empty());
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    fn lab_fav(state: &AppState, provider: &str, model: &str) {
        state.db.conn().execute("UPDATE lab_models SET favorite = 1 WHERE key = ?1", [provider_key(provider, model)]).unwrap();
    }

    #[tokio::test]
    async fn removing_a_model_only_touches_local_ollama_or_downloaded_files() {
        let server = serve(|req| match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/api/tags") => Resp::json(json!({"models": [{"name": "gone:1", "size": 5}]})),
            ("POST", "/api/show") => Resp::json(json!({})),
            ("DELETE", "/api/delete") => Resp::json(json!({})),
            ("GET", "/v1/models") => Resp::json(json!({"data": [{"id": "remote-model"}]})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "local", "ollama", &server.url);
        add_provider(&state, "other", "openai", &format!("{}/v1", server.url));
        let dir = storage::dir(&state).unwrap();
        fs::write(dir.join("mine.gguf"), gguf::testfile::sample()).unwrap();
        refresh(&state).await.unwrap();

        assert!(remove(&state, &provider_key("other", "remote-model")).await.unwrap_err().to_string().contains("cannot remove"));
        assert!(remove(&state, &file_key("mine.gguf")).await.unwrap_err().to_string().contains("cannot remove"));
        assert!(dir.join("mine.gguf").exists());
        assert!(remove(&state, "nope").await.is_err());

        remove(&state, &provider_key("local", "gone:1")).await.unwrap();
        let sent = server.last("/api/delete");
        assert_eq!((sent.method.as_str(), sent.json()["model"].as_str()), ("DELETE", Some("gone:1")));
        assert!(!list(&state).unwrap().iter().any(|m| m.model_id == "gone:1"));
        fs::remove_dir_all(&state.data_dir).unwrap();
    }
}
