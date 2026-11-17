//! Model downloads. Every download is started by an explicit request from the user.
//!
//! Two kinds:
//! - a GGUF file over HTTPS into the model folder: written to a `.part` file, resumed with an
//!   HTTP range request, hashed with SHA-256 while it arrives, checked to really be a GGUF
//!   file, and only then renamed into place;
//! - a pull through a connected Ollama, which downloads, resumes and verifies on its own;
//!   NEXUS relays its progress and can cancel it.
//!
//! The Hugging Face functions only list what is published there (repositories that contain
//! GGUF files, and each file's size and SHA-256); they are a search, not a download.

use super::{cancelled, gguf, storage, Lines};
use crate::ai;
use crate::error::{AppError, Result};
use crate::{activity, db, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::watch;

pub const EVENT_DOWNLOAD: &str = "nexus://lab-download";
pub const HF_BASE: &str = "https://huggingface.co";

/// Space kept free beyond the size of the download itself.
const DISK_MARGIN: u64 = 64 * 1024 * 1024;
const STALL_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_ACTIVE: usize = 3;

#[derive(Serialize, Clone, Debug)]
pub struct Download {
    pub id: String,
    /// "file" (into the model folder) or "ollama" (pulled by an Ollama connection).
    pub kind: String,
    pub source: String,
    pub name: String,
    pub provider_id: Option<String>,
    pub dest: Option<String>,
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    /// The SHA-256 the file is expected to have, when one is known.
    pub sha256: Option<String>,
    /// True once the content was checked against a checksum (by NEXUS, or by Ollama for a pull).
    pub verified: bool,
    /// "downloading", "verifying", "completed", "failed", "cancelled" or "interrupted".
    pub status: String,
    /// What is happening right now, in the runtime's own words where it has any.
    pub detail: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

const COLS: &str = "id, kind, source, name, provider_id, dest, total_bytes, received_bytes, sha256, verified, status, detail, error, created_at, updated_at, finished_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        kind: r.get(1)?,
        source: r.get(2)?,
        name: r.get(3)?,
        provider_id: r.get(4)?,
        dest: r.get(5)?,
        total_bytes: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        received_bytes: r.get::<_, i64>(7)? as u64,
        sha256: r.get(8)?,
        verified: r.get::<_, i64>(9)? != 0,
        status: r.get(10)?,
        detail: r.get(11)?,
        error: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
        finished_at: r.get(15)?,
    })
}

pub fn load(conn: &Connection, id: &str) -> Result<Download> {
    conn.query_row(&format!("SELECT {COLS} FROM lab_downloads WHERE id = ?1"), [id], row)
        .optional()?
        .ok_or_else(|| AppError::Msg("Download not found".into()))
}

pub fn list(conn: &Connection) -> Result<Vec<Download>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM lab_downloads ORDER BY created_at DESC, rowid DESC"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn is_active(status: &str) -> bool {
    matches!(status, "downloading" | "verifying")
}

fn set_progress(conn: &Connection, id: &str, received: u64, total: Option<u64>, detail: &str) {
    let _ = conn.execute(
        "UPDATE lab_downloads SET received_bytes = ?2, total_bytes = COALESCE(?3, total_bytes), detail = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, received as i64, total.map(|t| t as i64), detail, db::now()],
    );
}

fn set_status(conn: &Connection, id: &str, status: &str, error: Option<&str>) {
    let finished = (!is_active(status)).then(db::now);
    let _ = conn.execute(
        "UPDATE lab_downloads SET status = ?2, error = ?3, updated_at = ?4, finished_at = ?5 WHERE id = ?1",
        params![id, status, error, db::now(), finished],
    );
}

// ------------------------------------------------------------------ validation

/// A download address: HTTPS, or plain HTTP only for this machine. No credentials in the URL.
pub fn validate_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| AppError::Msg("That is not a valid URL".into()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Remove the user name and password from the URL".into());
    }
    if url.host_str().unwrap_or("").is_empty() {
        return Err("The URL needs a host name".into());
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if ai::is_local_url(url.as_str()) => Ok(url),
        "http" => Err("Downloads must use https:// (plain http:// is only accepted for this machine)".into()),
        _ => Err("Only https:// download addresses are supported".into()),
    }
}

pub fn validate_sha256(raw: Option<&str>) -> Result<Option<String>> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) => Ok(Some(s.to_ascii_lowercase())),
        Some(_) => Err("A SHA-256 checksum is 64 hexadecimal characters".into()),
    }
}

fn valid_ollama_name(name: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._/:-]{0,199}$").unwrap()).is_match(name)
}

fn valid_repo(repo: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,95}/[A-Za-z0-9][A-Za-z0-9._-]{0,95}$").unwrap()).is_match(repo)
        && !repo.contains("..")
}

/// A client for file downloads: redirects may only lead to HTTPS (or stay on this machine).
fn download_client() -> Result<reqwest::Client> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        let url = attempt.url();
        if url.scheme() == "https" || ai::is_local_url(url.as_str()) {
            attempt.follow()
        } else {
            attempt.error("the server redirected to an address that is not HTTPS")
        }
    });
    reqwest::Client::builder()
        .user_agent(concat!("nexus/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .redirect(policy)
        .build()
        .map_err(|e| AppError::Msg(format!("Could not prepare the download: {e}")))
}

// ------------------------------------------------------------------ Hugging Face listing

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct HfModel {
    pub id: String,
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    pub license: Option<String>,
    pub gated: bool,
    pub updated_at: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct HfFile {
    pub path: String,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub url: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct HfRepo {
    pub id: String,
    pub license: Option<String>,
    pub gated: bool,
    pub architecture: Option<String>,
    pub context_length: Option<u64>,
    pub updated_at: Option<String>,
    pub page_url: String,
    pub files: Vec<HfFile>,
}

fn license_of(model: &Value) -> Option<String> {
    model.pointer("/cardData/license").and_then(|l| l.as_str()).map(String::from).or_else(|| {
        model
            .get("tags")
            .and_then(|t| t.as_array())?
            .iter()
            .filter_map(|t| t.as_str())
            .find_map(|t| t.strip_prefix("license:"))
            .map(String::from)
    })
}

fn gated(model: &Value) -> bool {
    match model.get("gated") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty() && s != "false",
        _ => false,
    }
}

pub fn parse_hf_search(list: &Value) -> Vec<HfModel> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(HfModel {
                id: m.get("id").and_then(|i| i.as_str())?.to_string(),
                downloads: m.get("downloads").and_then(|v| v.as_u64()),
                likes: m.get("likes").and_then(|v| v.as_u64()),
                license: license_of(m),
                gated: gated(m),
                updated_at: m.get("lastModified").or(m.get("createdAt")).and_then(|v| v.as_str()).map(String::from),
            })
        })
        .collect()
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn parse_hf_repo(base: &str, repo: &str, info: &Value, tree: &Value) -> HfRepo {
    let mut files: Vec<HfFile> = tree
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f.get("type").and_then(|t| t.as_str()) == Some("file"))
        .filter_map(|f| {
            let path = f.get("path").and_then(|p| p.as_str())?;
            if !path.to_ascii_lowercase().ends_with(".gguf") {
                return None;
            }
            Some(HfFile {
                path: path.to_string(),
                size_bytes: f.pointer("/lfs/size").or(f.get("size")).and_then(|v| v.as_u64()),
                sha256: f.pointer("/lfs/oid").and_then(|v| v.as_str()).filter(|s| s.len() == 64).map(String::from),
                url: format!("{base}/{repo}/resolve/main/{}", encode_path(path)),
            })
        })
        .collect();
    files.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then(a.path.cmp(&b.path)));
    HfRepo {
        id: repo.to_string(),
        license: license_of(info),
        gated: gated(info),
        architecture: info.pointer("/gguf/architecture").and_then(|v| v.as_str()).map(String::from),
        context_length: info.pointer("/gguf/context_length").and_then(|v| v.as_u64()),
        updated_at: info.get("lastModified").and_then(|v| v.as_str()).map(String::from),
        page_url: format!("{base}/{repo}"),
        files,
    }
}

async fn hf_get(state: &AppState, request: reqwest::RequestBuilder) -> Result<Value> {
    let _ = state;
    let resp = request.timeout(Duration::from_secs(20)).send().await.map_err(|e| {
        AppError::Msg(if e.is_connect() || e.is_timeout() {
            "Could not reach Hugging Face. Check the internet connection; everything else in Model Lab works offline.".into()
        } else {
            format!("Hugging Face request failed: {}", e.without_url())
        })
    })?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err("Hugging Face has no repository with that name".into());
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("That repository is private or gated; NEXUS only lists public repositories".into());
    }
    if !status.is_success() {
        return Err(AppError::Msg(format!("Hugging Face returned {status}")));
    }
    resp.json().await.map_err(|_| AppError::Msg("Hugging Face replied with something unexpected".into()))
}

/// Public repositories on Hugging Face that contain GGUF files and match `query`.
pub async fn hf_search(state: &AppState, base: &str, query: &str) -> Result<Vec<HfModel>> {
    let query = query.trim();
    if query.is_empty() || query.len() > 100 {
        return Err("Enter a model name to search for".into());
    }
    let request = state.http.get(format!("{base}/api/models")).query(&[
        ("search", query),
        ("filter", "gguf"),
        ("sort", "downloads"),
        ("direction", "-1"),
        ("limit", "40"),
    ]);
    Ok(parse_hf_search(&hf_get(state, request).await?))
}

/// The GGUF files of one repository, with the size and SHA-256 Hugging Face publishes for each.
pub async fn hf_files(state: &AppState, base: &str, repo: &str) -> Result<HfRepo> {
    let repo = repo.trim().trim_matches('/');
    if !valid_repo(repo) {
        return Err("A repository is written as owner/name".into());
    }
    let info = hf_get(state, state.http.get(format!("{base}/api/models/{repo}"))).await?;
    let tree = hf_get(state, state.http.get(format!("{base}/api/models/{repo}/tree/main")).query(&[("recursive", "true")])).await?;
    Ok(parse_hf_repo(base, repo, &info, &tree))
}

// ------------------------------------------------------------------ starting

/// Record a new file download, or find the unfinished one it continues.
pub fn prepare_file(state: &AppState, url: &str, name: Option<&str>, sha256: Option<&str>) -> Result<Download> {
    let url = validate_url(url)?;
    let sha256 = validate_sha256(sha256)?;
    let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => storage::safe_name(n)?,
        None => storage::name_from_source(url.path())?,
    };
    let dir = storage::dir(state)?;
    if !dir.is_dir() {
        return Err("The model folder does not exist. Choose a folder in Model Lab → Library → Storage.".into());
    }
    let dest = dir.join(&name);
    if dest.exists() {
        return Err(AppError::Msg(format!("{name} is already in the model folder")));
    }
    let conn = state.db.conn();
    let existing: Option<Download> = conn
        .query_row(&format!("SELECT {COLS} FROM lab_downloads WHERE kind = 'file' AND dest = ?1 AND status != 'completed' ORDER BY created_at DESC LIMIT 1"), [dest.to_string_lossy()], row)
        .optional()?;
    if let Some(d) = existing {
        if is_active(&d.status) {
            return Err(AppError::Msg(format!("{name} is already downloading")));
        }
        if d.source != url.as_str() {
            return Err(AppError::Msg(format!("An unfinished download of {name} from another address exists. Resume or remove it first.")));
        }
        return Ok(d);
    }
    let id = db::new_id();
    let now = db::now();
    conn.execute(
        "INSERT INTO lab_downloads (id, kind, source, name, dest, sha256, status, detail, created_at, updated_at)
         VALUES (?1, 'file', ?2, ?3, ?4, ?5, 'downloading', 'Starting', ?6, ?6)",
        params![id, url.as_str(), name, dest.to_string_lossy(), sha256, now],
    )?;
    activity::log(&conn, "models", &format!("Started downloading {name}"), None);
    load(&conn, &id)
}

pub fn prepare_ollama(state: &AppState, provider_id: &str, model: &str) -> Result<Download> {
    let provider = ai::usable_provider(state, provider_id)?;
    if provider.kind != "ollama" {
        return Err("Only an Ollama connection can pull models by name".into());
    }
    let model = model.trim();
    if !valid_ollama_name(model) {
        return Err("Enter an Ollama model name such as name:tag".into());
    }
    let conn = state.db.conn();
    let running: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM lab_downloads WHERE kind = 'ollama' AND provider_id = ?1 AND name = ?2 AND status IN ('downloading', 'verifying'))",
        params![provider_id, model],
        |r| r.get(0),
    )?;
    if running {
        return Err(AppError::Msg(format!("{model} is already being pulled")));
    }
    // One record per model and connection: a repeated pull continues the previous one.
    conn.execute("DELETE FROM lab_downloads WHERE kind = 'ollama' AND provider_id = ?1 AND name = ?2", params![provider_id, model])?;
    let id = db::new_id();
    let now = db::now();
    conn.execute(
        "INSERT INTO lab_downloads (id, kind, source, name, provider_id, status, detail, created_at, updated_at)
         VALUES (?1, 'ollama', ?2, ?3, ?4, 'downloading', 'Starting', ?5, ?5)",
        params![id, format!("Ollama registry via {}", provider.name), model, provider_id, now],
    )?;
    activity::log(&conn, "models", &format!("Started pulling {model} with {}", provider.name), None);
    load(&conn, &id)
}

// ------------------------------------------------------------------ running

enum Outcome {
    Completed,
    Cancelled,
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(storage::PART_SUFFIX);
    dest.with_file_name(name)
}

fn hash_file(path: &Path, hasher: &mut Sha256) -> Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn run_file(
    state: &AppState,
    d: &Download,
    cancel: &mut watch::Receiver<bool>,
    notify: &(dyn Fn(&str) + Send + Sync),
) -> Result<Outcome> {
    let dest = PathBuf::from(d.dest.as_deref().ok_or("This download has no destination")?);
    let part = part_path(&dest);
    if dest.exists() {
        return Err(AppError::Msg(format!("{} is already in the model folder", d.name)));
    }
    let url = validate_url(&d.source)?;
    let client = download_client()?;
    let mut existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut request = client.get(url.clone());
    if existing > 0 {
        request = request.header("Range", format!("bytes={existing}-"));
    }
    let send = |request: reqwest::RequestBuilder| async move {
        request.send().await.map_err(|e| {
            AppError::Msg(if e.is_connect() {
                "Could not connect to the download server".to_string()
            } else {
                format!("The download failed: {}", e.without_url())
            })
        })
    };
    let mut resp = send(request).await?;
    if resp.status().as_u16() == 416 {
        // The partial file does not match what the server has; start again from nothing.
        let _ = fs::remove_file(&part);
        existing = 0;
        resp = send(client.get(url)).await?;
    }
    let status = resp.status();
    match status.as_u16() {
        200 => existing = 0, // the server sent the whole file (it ignored or was not asked for a range)
        206 => {
            let starts_here = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim_start_matches("bytes ").split('-').next().and_then(|n| n.trim().parse::<u64>().ok()));
            if starts_here != Some(existing) {
                return Err("The server resumed at the wrong position; remove this download and start it again".into());
            }
        }
        401 | 403 => return Err("The server refused the download (it may need a login NEXUS does not have)".into()),
        404 => return Err("The server has no file at that address".into()),
        _ => return Err(AppError::Msg(format!("The download server returned {status}"))),
    }
    let total = resp.content_length().map(|len| existing + len);
    if let (Some(total), Some((free, _))) = (total, storage::disk_space(&dest)) {
        let needed = total - existing;
        if free < needed.saturating_add(DISK_MARGIN) {
            return Err(AppError::Msg(format!(
                "Not enough disk space: this needs {:.1} GB more and {:.1} GB is free",
                needed as f64 / 1e9,
                free as f64 / 1e9
            )));
        }
    }

    let mut hasher = Sha256::new();
    let mut file = if existing > 0 {
        notify("Checking the part already downloaded");
        hash_file(&part, &mut hasher)?;
        OpenOptions::new().append(true).open(&part)?
    } else {
        OpenOptions::new().create(true).write(true).truncate(true).open(&part)?
    };
    let mut received = existing;
    set_progress(&state.db.conn(), &d.id, received, total, "Downloading");
    notify("Downloading");
    let mut last_saved = Instant::now();
    loop {
        let chunk = tokio::select! {
            chunk = tokio::time::timeout(STALL_TIMEOUT, resp.chunk()) => chunk,
            _ = cancel.changed() => {
                drop(file);
                let _ = fs::remove_file(&part);
                return Ok(Outcome::Cancelled);
            }
        };
        let chunk = match chunk {
            Err(_) => return Err("The download stalled. Resume it to continue from where it stopped.".into()),
            Ok(Err(e)) => return Err(AppError::Msg(format!("The connection was lost: {}. Resume to continue.", e.without_url()))),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => bytes,
        };
        file.write_all(&chunk).map_err(|e| {
            AppError::Msg(if e.raw_os_error() == Some(28) {
                "The disk is full. Free some space and resume.".into()
            } else {
                format!("Could not write the file: {e}")
            })
        })?;
        hasher.update(&chunk);
        received += chunk.len() as u64;
        if last_saved.elapsed() >= Duration::from_millis(400) {
            last_saved = Instant::now();
            set_progress(&state.db.conn(), &d.id, received, total, "Downloading");
            notify("Downloading");
        }
    }
    file.flush()?;
    file.sync_all()?;
    drop(file);
    set_progress(&state.db.conn(), &d.id, received, total, "Verifying");
    if total.is_some_and(|t| t != received) {
        return Err("The connection closed before the file was complete. Resume to continue.".into());
    }

    set_status(&state.db.conn(), &d.id, "verifying", None);
    notify("Verifying");
    let actual = hex(&hasher.finalize());
    if let Some(expected) = &d.sha256 {
        if &actual != expected {
            let _ = fs::remove_file(&part);
            return Err(AppError::Msg(format!("Checksum mismatch: expected {expected}, got {actual}. The download was discarded.")));
        }
    }
    // Whatever the server sent, only a real GGUF file is kept.
    if let Err(e) = gguf::read(&part) {
        let _ = fs::remove_file(&part);
        return Err(AppError::Msg(format!("What was downloaded is not a usable GGUF model file ({e}). It was discarded.")));
    }
    if dest.exists() {
        return Err(AppError::Msg(format!("{} appeared in the model folder during the download; the partial file was kept", d.name)));
    }
    fs::rename(&part, &dest)?;
    let conn = state.db.conn();
    conn.execute(
        "UPDATE lab_downloads SET verified = ?2, sha256 = COALESCE(sha256, ?3), received_bytes = ?4, total_bytes = ?4 WHERE id = ?1",
        params![d.id, d.sha256.is_some() as i64, actual, received as i64],
    )?;
    Ok(Outcome::Completed)
}

/// One line of Ollama's pull progress folded into the running totals.
#[derive(Default)]
struct PullProgress {
    layers: HashMap<String, (u64, u64)>,
    detail: String,
    verified: bool,
    done: bool,
}

impl PullProgress {
    fn apply(&mut self, line: &Value) -> Result<()> {
        if let Some(error) = line.get("error").and_then(|e| e.as_str()) {
            return Err(AppError::Msg(format!("Ollama could not pull the model: {error}")));
        }
        let status = line.get("status").and_then(|s| s.as_str()).unwrap_or("");
        if let Some(digest) = line.get("digest").and_then(|d| d.as_str()) {
            let entry = self.layers.entry(digest.to_string()).or_default();
            if let Some(total) = line.get("total").and_then(|v| v.as_u64()) {
                entry.0 = total;
            }
            if let Some(done) = line.get("completed").and_then(|v| v.as_u64()) {
                entry.1 = done;
            }
        }
        if status.contains("verifying") {
            self.verified = true;
        }
        if status == "success" {
            self.done = true;
        }
        if !status.is_empty() {
            self.detail = if status.starts_with("pulling ") && line.get("digest").is_some() {
                "Downloading layers".into()
            } else {
                status.to_string()
            };
        }
        Ok(())
    }

    fn totals(&self) -> (u64, Option<u64>) {
        let total: u64 = self.layers.values().map(|l| l.0).sum();
        (self.layers.values().map(|l| l.1).sum(), (total > 0).then_some(total))
    }
}

async fn run_ollama(
    state: &AppState,
    d: &Download,
    cancel: &mut watch::Receiver<bool>,
    notify: &(dyn Fn(&str) + Send + Sync),
) -> Result<Outcome> {
    let provider = ai::usable_provider(state, d.provider_id.as_deref().unwrap_or(""))?;
    let body = json!({ "model": d.name, "stream": true });
    let ai::Authed { req, redactor } = ai::authed(&provider, state.http.post(format!("{}/api/pull", provider.base_url)).json(&body))?;
    let mut resp = req.send().await.map_err(|e| ai::transport_error(e, &redactor))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ai::error_detail(status, &body, &redactor));
    }
    let mut lines = Lines::default();
    let mut progress = PullProgress::default();
    let mut last_saved = Instant::now();
    loop {
        let chunk = tokio::select! {
            chunk = tokio::time::timeout(STALL_TIMEOUT, resp.chunk()) => chunk,
            // Closing the connection is how a pull is cancelled; Ollama keeps what it has.
            _ = cancel.changed() => return Ok(Outcome::Cancelled),
        };
        let pieces = match chunk {
            Err(_) => return Err("Ollama stopped reporting progress. Pull again to continue; it resumes where it stopped.".into()),
            Ok(Err(e)) => return Err(ai::transport_error(e, &redactor)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => lines.push(&bytes),
        };
        for line in pieces.iter().filter(|l| !l.trim().is_empty()) {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                progress.apply(&value)?;
            }
        }
        if last_saved.elapsed() >= Duration::from_millis(400) {
            last_saved = Instant::now();
            let (received, total) = progress.totals();
            set_progress(&state.db.conn(), &d.id, received, total, &progress.detail);
            notify(&progress.detail);
        }
    }
    if let Some(rest) = lines.finish() {
        if let Ok(value) = serde_json::from_str::<Value>(&rest) {
            progress.apply(&value)?;
        }
    }
    let (received, total) = progress.totals();
    set_progress(&state.db.conn(), &d.id, received, total, &progress.detail);
    if !progress.done {
        return Err("Ollama ended the pull without reporting success. Pull again to continue.".into());
    }
    state.db.conn().execute("UPDATE lab_downloads SET verified = ?2 WHERE id = ?1", params![d.id, progress.verified as i64])?;
    Ok(Outcome::Completed)
}

/// Run a prepared download to its end and record the result. `notify` is called whenever the
/// stored progress changed.
pub async fn run(state: &AppState, id: &str, mut cancel: watch::Receiver<bool>, notify: &(dyn Fn(&str) + Send + Sync)) -> Download {
    // Loaded first so the database lock is released before anything is awaited.
    let loaded = load(&state.db.conn(), id);
    let outcome = match loaded {
        Ok(d) => {
            set_status(&state.db.conn(), id, "downloading", None);
            let result = if d.kind == "ollama" {
                run_ollama(state, &d, &mut cancel, notify).await
            } else {
                run_file(state, &d, &mut cancel, notify).await
            };
            // A cancel that raced with a failure is still a cancel.
            match result {
                Err(_) if cancelled(&cancel) => {
                    if let Some(dest) = d.dest.as_deref() {
                        let _ = fs::remove_file(part_path(Path::new(dest)));
                    }
                    Ok(Outcome::Cancelled)
                }
                other => other,
            }
        }
        Err(e) => Err(e),
    };
    {
        let conn = state.db.conn();
        match &outcome {
            Ok(Outcome::Completed) => {
                set_status(&conn, id, "completed", None);
                let _ = conn.execute("UPDATE lab_downloads SET detail = 'Done' WHERE id = ?1", [id]);
                if let Ok(d) = load(&conn, id) {
                    activity::log(&conn, "models", &format!("Finished downloading {}", d.name), None);
                }
            }
            Ok(Outcome::Cancelled) => {
                set_status(&conn, id, "cancelled", None);
                let _ = conn.execute("UPDATE lab_downloads SET detail = 'Cancelled', received_bytes = CASE kind WHEN 'file' THEN 0 ELSE received_bytes END WHERE id = ?1", [id]);
            }
            Err(e) => set_status(&conn, id, "failed", Some(&e.to_string())),
        }
    }
    state.lab.downloads.finish(id);
    notify("finished");
    load(&state.db.conn(), id).unwrap_or_else(|_| Download {
        id: id.to_string(),
        kind: String::new(),
        source: String::new(),
        name: String::new(),
        provider_id: None,
        dest: None,
        total_bytes: None,
        received_bytes: 0,
        sha256: None,
        verified: false,
        status: "failed".into(),
        detail: String::new(),
        error: Some("Download not found".into()),
        created_at: String::new(),
        updated_at: String::new(),
        finished_at: None,
    })
}

/// Start (or continue) a prepared download in the background.
fn spawn<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<()> {
    let state = app.state::<AppState>();
    if state.lab.downloads.count() >= MAX_ACTIVE {
        return Err(AppError::Msg(format!("{MAX_ACTIVE} downloads are already running. Wait for one to finish.")));
    }
    let cancel = state.lab.downloads.register(id)?;
    set_status(&state.db.conn(), id, "downloading", None);
    // A restarted download must not keep showing how the previous attempt ended.
    let _ = state.db.conn().execute("UPDATE lab_downloads SET detail = 'Starting' WHERE id = ?1", [id]);
    let (app, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let emitter = app.clone();
        let event_id = id.clone();
        let notify = move |_: &str| {
            let _ = emitter.emit(EVENT_DOWNLOAD, &event_id);
        };
        run(&state, &id, cancel, &notify).await;
    });
    Ok(())
}

// ------------------------------------------------------------------ commands

#[tauri::command]
pub async fn lab_hf_search(state: State<'_, AppState>, query: String) -> Result<Vec<HfModel>> {
    hf_search(&state, HF_BASE, &query).await
}

#[tauri::command]
pub async fn lab_hf_files(state: State<'_, AppState>, repo: String) -> Result<HfRepo> {
    hf_files(&state, HF_BASE, &repo).await
}

#[tauri::command]
pub fn lab_downloads(state: State<'_, AppState>) -> Result<Vec<Download>> {
    list(&state.db.conn())
}

#[tauri::command]
pub fn lab_download_file<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    url: String,
    name: Option<String>,
    sha256: Option<String>,
) -> Result<Download> {
    let d = prepare_file(&state, &url, name.as_deref(), sha256.as_deref())?;
    spawn(&app, &d.id).inspect_err(|_| {
        // A download that could not start leaves no half-made record behind.
        let _ = state.db.conn().execute("DELETE FROM lab_downloads WHERE id = ?1 AND received_bytes = 0", [&d.id]);
    })?;
    load(&state.db.conn(), &d.id)
}

#[tauri::command]
pub fn lab_download_ollama<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    provider_id: String,
    model: String,
) -> Result<Download> {
    let d = prepare_ollama(&state, &provider_id, &model)?;
    spawn(&app, &d.id).inspect_err(|_| {
        let _ = state.db.conn().execute("DELETE FROM lab_downloads WHERE id = ?1", [&d.id]);
    })?;
    load(&state.db.conn(), &d.id)
}

/// Continue a download that failed, was interrupted or was cancelled.
#[tauri::command]
pub fn lab_download_resume<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, id: String) -> Result<Download> {
    let d = load(&state.db.conn(), &id)?;
    if d.status == "completed" {
        return Err("This download already finished".into());
    }
    if is_active(&d.status) || state.lab.downloads.is_running(&id) {
        return Err("This download is already running".into());
    }
    spawn(&app, &id)?;
    load(&state.db.conn(), &id)
}

#[tauri::command]
pub fn lab_download_cancel(state: State<'_, AppState>, id: String) -> Result<()> {
    if !state.lab.downloads.cancel(&id) {
        // Nothing is running under this id; make sure the record does not claim otherwise.
        let d = load(&state.db.conn(), &id)?;
        if is_active(&d.status) {
            set_status(&state.db.conn(), &id, "interrupted", Some("The download is no longer running. Resume to continue."));
        }
    }
    Ok(())
}

/// Remove a download from the list. An unfinished file download's partial file goes with it;
/// a finished model file is never touched here.
#[tauri::command]
pub fn lab_download_remove(state: State<'_, AppState>, id: String) -> Result<()> {
    let d = load(&state.db.conn(), &id)?;
    if is_active(&d.status) || state.lab.downloads.is_running(&id) {
        return Err("Cancel the download before removing it".into());
    }
    if d.kind == "file" && d.status != "completed" {
        if let Some(dest) = &d.dest {
            let _ = fs::remove_file(part_path(Path::new(dest)));
        }
    }
    // The record of a finished file download is what marks the file as NEXUS's own; keep it
    // until the file itself is deleted through the model folder.
    if d.kind == "file" && d.status == "completed" && d.dest.as_deref().is_some_and(|p| Path::new(p).exists()) {
        return Err("This file is in the model folder. Delete the model there to remove it.".into());
    }
    state.db.conn().execute("DELETE FROM lab_downloads WHERE id = ?1", [&id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testserver::{serve, Req, Resp};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn quiet(_: &str) {}

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// Serves `bytes` at any path, honouring range requests like a real file server.
    fn file_server(bytes: Vec<u8>) -> impl Fn(&Req) -> Resp + Send + Sync + 'static {
        move |req| match req
            .headers
            .get("range")
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
        {
            Some(start) if start < bytes.len() => Resp::bytes(206, "application/octet-stream", bytes[start..].to_vec())
                .header("Content-Range", &format!("bytes {start}-{}/{}", bytes.len() - 1, bytes.len())),
            Some(_) => Resp::bytes(416, "text/plain", vec![]),
            None => Resp::bytes(200, "application/octet-stream", bytes.clone()),
        }
    }

    async fn download(state: &AppState, url: &str, name: Option<&str>, sha256: Option<&str>) -> Download {
        let d = prepare_file(state, url, name, sha256).unwrap();
        let cancel = state.lab.downloads.register(&d.id).unwrap();
        run(state, &d.id, cancel, &quiet).await
    }

    #[test]
    fn download_addresses_and_checksums_are_validated() {
        assert!(validate_url("https://huggingface.co/a/b/resolve/main/x.gguf").is_ok());
        assert!(validate_url("http://127.0.0.1:8000/x.gguf").is_ok());
        for bad in [
            "http://example.com/x.gguf",
            "file:///etc/passwd",
            "ftp://host/x.gguf",
            "https://user:pw@host/x.gguf",
            "not a url",
            "/local/path.gguf",
            "javascript:alert(1)",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
        let digest = "A".repeat(64);
        assert_eq!(validate_sha256(Some(&digest)).unwrap(), Some("a".repeat(64)));
        assert_eq!(validate_sha256(Some("  ")).unwrap(), None);
        assert!(validate_sha256(Some("abc")).is_err());
        assert!(validate_sha256(Some(&"g".repeat(64))).is_err());
        assert!(valid_ollama_name("qwen2.5-coder:1.5b") && valid_ollama_name("library/llama3.2:latest"));
        for bad in ["", "-flag", "a b", "a;rm -rf", "$(x)", &"a".repeat(300)] {
            assert!(!valid_ollama_name(bad), "{bad}");
        }
        assert!(
            valid_repo("ggml-org/tiny-llamas")
                && !valid_repo("nope")
                && !valid_repo("a/b/c")
                && !valid_repo("../x/y")
                && !valid_repo("a/..b")
        );
    }

    #[tokio::test]
    async fn a_file_is_downloaded_verified_and_moved_into_place() {
        let model = gguf::testfile::sample();
        let server = serve(file_server(model.clone())).await;
        let state = AppState::for_tests();
        let dir = storage::dir(&state).unwrap();
        let url = format!("{}/org/repo/resolve/main/tiny.gguf", server.url);

        let done = download(&state, &url, None, Some(&sha(&model))).await;
        assert_eq!((done.status.as_str(), done.verified, done.error.as_deref()), ("completed", true, None));
        assert_eq!((done.received_bytes, done.total_bytes), (model.len() as u64, Some(model.len() as u64)));
        assert_eq!(fs::read(dir.join("tiny.gguf")).unwrap(), model);
        assert!(!dir.join("tiny.gguf.part").exists(), "no temporary file is left behind");
        assert!(storage::is_managed(&state, &dir.join("tiny.gguf")));
        assert_eq!(state.lab.downloads.count(), 0);

        // The same file is not downloaded twice.
        assert!(prepare_file(&state, &url, None, None).unwrap_err().to_string().contains("already in the model folder"));
        // Without a published checksum the file is still hashed, and says it was not verified.
        let unverified = download(&state, &url, Some("copy.gguf"), None).await;
        assert_eq!((unverified.status.as_str(), unverified.verified), ("completed", false));
        assert_eq!(unverified.sha256.as_deref(), Some(sha(&model).as_str()), "the computed hash is recorded");
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn wrong_checksums_and_non_model_content_are_discarded() {
        let model = gguf::testfile::sample();
        let server = serve(file_server(model.clone())).await;
        let page = serve(|_| Resp::bytes(200, "text/html", b"<html>Sign in to continue</html>".to_vec())).await;
        let state = AppState::for_tests();
        let dir = storage::dir(&state).unwrap();

        let bad = download(&state, &format!("{}/a.gguf", server.url), None, Some(&"0".repeat(64))).await;
        assert_eq!(bad.status, "failed");
        assert!(bad.error.as_deref().unwrap().contains("Checksum mismatch"));
        let html = download(&state, &format!("{}/b.gguf", page.url), None, None).await;
        assert_eq!(html.status, "failed");
        assert!(html.error.as_deref().unwrap().contains("not a usable GGUF"));
        let empty = serve(|_| Resp::error(404, json!({}))).await;
        let missing = download(&state, &format!("{}/c.gguf", empty.url), None, None).await;
        assert!(missing.error.as_deref().unwrap().contains("no file at that address"));
        let offline = download(&state, "http://127.0.0.1:9/d.gguf", None, None).await;
        assert!(offline.error.as_deref().unwrap().contains("Could not connect"));
        let listing: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert!(listing.is_empty(), "nothing rejected stays on disk: {listing:?}");
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn an_interrupted_download_resumes_from_where_it_stopped() {
        let mut model = gguf::testfile::sample();
        model.extend(std::iter::repeat_n(7u8, 300_000));
        let half = model.len() / 2;
        let (first, calls) = (model[..half].to_vec(), Arc::new(AtomicUsize::new(0)));
        let (full, seen) = (model.clone(), calls.clone());
        let server = serve(move |req| {
            // The first request dies halfway: the server promises the whole file and sends half.
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                return Resp {
                    status: 200,
                    headers: vec![("Content-Length".into(), full.len().to_string())],
                    body: super::super::testserver::Body::Stream(vec![(0, first.clone())]),
                };
            }
            file_server(full.clone())(req)
        })
        .await;
        let state = AppState::for_tests();
        let dir = storage::dir(&state).unwrap();
        let url = format!("{}/big.gguf", server.url);

        let broken = download(&state, &url, None, Some(&sha(&model))).await;
        assert_eq!(broken.status, "failed");
        assert!(broken.error.as_deref().unwrap().to_lowercase().contains("resume"), "{:?}", broken.error);
        assert_eq!(fs::metadata(dir.join("big.gguf.part")).unwrap().len(), half as u64, "the partial file is kept");
        assert!(!dir.join("big.gguf").exists());

        // Asking for the same file again continues the same download rather than starting another.
        let again = prepare_file(&state, &url, None, None).unwrap();
        assert_eq!(again.id, broken.id);
        let cancel = state.lab.downloads.register(&again.id).unwrap();
        let done = run(&state, &again.id, cancel, &quiet).await;
        assert_eq!((done.status.as_str(), done.verified), ("completed", true));
        assert_eq!(server.last("/big.gguf").headers.get("range").map(String::as_str), Some(format!("bytes={half}-").as_str()));
        assert_eq!(fs::read(dir.join("big.gguf")).unwrap(), model, "the two halves make the exact file");
        assert_eq!(list(&state.db.conn()).unwrap().len(), 1);
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn cancelling_stops_the_transfer_and_removes_the_partial_file() {
        let head = gguf::testfile::sample();
        let server = serve(move |_| Resp {
            status: 200,
            headers: vec![("Content-Length".into(), "50000000".into())],
            body: super::super::testserver::Body::StreamThenHang(vec![(0, head.clone())]),
        })
        .await;
        let state = AppState::for_tests();
        let dir = storage::dir(&state).unwrap();
        let d = prepare_file(&state, &format!("{}/slow.gguf", server.url), None, None).unwrap();
        let cancel = state.lab.downloads.register(&d.id).unwrap();
        let (result, _) = tokio::join!(run(&state, &d.id, cancel, &quiet), async {
            let started = Instant::now();
            while fs::metadata(dir.join("slow.gguf.part")).map(|m| m.len()).unwrap_or(0) == 0 {
                assert!(started.elapsed() < Duration::from_secs(10), "the download never started");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(state.lab.downloads.cancel(&d.id));
        });
        assert_eq!((result.status.as_str(), result.error.as_deref()), ("cancelled", None));
        assert!(!dir.join("slow.gguf.part").exists() && !dir.join("slow.gguf").exists());
        assert_eq!(state.lab.downloads.count(), 0);
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn a_download_larger_than_the_free_disk_space_is_refused_before_writing() {
        // Claims to be far larger than any disk; no body needs to be sent for the check.
        let server = serve(|_| Resp {
            status: 200,
            headers: vec![("Content-Length".into(), u64::MAX.to_string().chars().take(18).collect())],
            body: super::super::testserver::Body::Stream(vec![]),
        })
        .await;
        let state = AppState::for_tests();
        let d = download(&state, &format!("{}/huge.gguf", server.url), None, None).await;
        #[cfg(unix)]
        {
            assert_eq!(d.status, "failed");
            assert!(d.error.as_deref().unwrap().contains("Not enough disk space"), "{:?}", d.error);
        }
        let _ = d;
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn redirects_may_not_leave_https() {
        let target = serve(file_server(gguf::testfile::sample())).await;
        let to_local = target.url.clone();
        let local_redirect =
            serve(move |_| Resp::bytes(302, "text/plain", vec![]).header("Location", &format!("{to_local}/real.gguf"))).await;
        let insecure_redirect =
            serve(|_| Resp::bytes(302, "text/plain", vec![]).header("Location", "http://example.invalid/evil.gguf")).await;
        let state = AppState::for_tests();
        let ok = download(&state, &format!("{}/a.gguf", local_redirect.url), None, None).await;
        assert_eq!(ok.status, "completed", "{:?}", ok.error);
        let refused = download(&state, &format!("{}/b.gguf", insecure_redirect.url), None, None).await;
        assert_eq!(refused.status, "failed");
        fs::remove_dir_all(&state.data_dir).unwrap();
    }

    #[tokio::test]
    async fn an_ollama_pull_relays_progress_and_reports_runtime_errors() {
        // Line shapes follow Ollama's /api/pull stream.
        let server = serve(|req| match (req.path.as_str(), req.json()["model"].as_str()) {
            ("/api/pull", Some("tiny:latest")) => Resp::stream("application/x-ndjson", vec![
                (0, "{\"status\":\"pulling manifest\"}\n".into()),
                (5, "{\"status\":\"pulling aaa\",\"digest\":\"sha256:aaa\",\"total\":1000,\"completed\":400}\n{\"status\":\"pulling bbb\",\"digest\":\"sha256:bbb\",\"total\":50,\"comp".into()),
                (5, "leted\":50}\n{\"status\":\"pulling aaa\",\"digest\":\"sha256:aaa\",\"total\":1000,\"completed\":1000}\n".into()),
                (5, "{\"status\":\"verifying sha256 digest\"}\n{\"status\":\"writing manifest\"}\n{\"status\":\"success\"}\n".into()),
            ]),
            ("/api/pull", Some("nope:latest")) => Resp::stream("application/x-ndjson", vec![(0, "{\"status\":\"pulling manifest\"}\n{\"error\":\"pull model manifest: file does not exist\"}\n".into())]),
            ("/api/pull", Some("cut:latest")) => Resp::stream("application/x-ndjson", vec![(0, "{\"status\":\"pulling aaa\",\"digest\":\"sha256:aaa\",\"total\":10,\"completed\":3}\n".into())]),
            _ => Resp::error(404, json!({"error": "unknown"})),
        }).await;
        let state = AppState::for_tests();
        state
            .db
            .conn()
            .execute(
                "INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES ('o', 'Ollama', 'ollama', ?1, 0, '')",
                [&server.url],
            )
            .unwrap();
        state
            .db
            .conn()
            .execute(
                "INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES ('g', 'Generic', 'openai', ?1, 0, '')",
                [&server.url],
            )
            .unwrap();
        let pull = |model: &'static str| {
            let state = &state;
            async move {
                let d = prepare_ollama(state, "o", model).unwrap();
                let cancel = state.lab.downloads.register(&d.id).unwrap();
                run(state, &d.id, cancel, &quiet).await
            }
        };
        let ok = pull("tiny:latest").await;
        assert_eq!((ok.status.as_str(), ok.verified, ok.detail.as_str()), ("completed", true, "Done"));
        assert_eq!((ok.received_bytes, ok.total_bytes), (1050, Some(1050)), "layers are added up across split lines");
        let missing = pull("nope:latest").await;
        assert_eq!(missing.status, "failed");
        assert!(missing.error.as_deref().unwrap().contains("file does not exist"));
        let cut = pull("cut:latest").await;
        assert!(cut.error.as_deref().unwrap().contains("without reporting success"));

        assert!(prepare_ollama(&state, "g", "tiny:latest").unwrap_err().to_string().contains("Only an Ollama connection"));
        assert!(prepare_ollama(&state, "o", "bad name; rm").is_err());
        // Pulling the same model again replaces its record instead of piling up.
        pull("tiny:latest").await;
        assert_eq!(list(&state.db.conn()).unwrap().iter().filter(|d| d.name == "tiny:latest").count(), 1);
        let _ = fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn hugging_face_listing_is_parsed_from_real_reply_shapes() {
        // Trimmed copies of real replies from huggingface.co.
        let server = serve(|req| {
            if req.path.starts_with("/api/models?") {
                Resp::json(json!([
                    {"_id": "664d", "id": "ggml-org/tiny-llamas", "likes": 4, "private": false, "downloads": 25300, "tags": ["gguf", "endpoints_compatible", "region:us"], "createdAt": "2024-05-22T13:14:01.000Z"},
                    {"id": "ngxson/tinyllama_split_test", "likes": 0, "downloads": 2531, "tags": ["gguf", "license:mit"], "createdAt": "2024-04-03T15:32:54.000Z", "gated": "auto"}
                ]))
            } else if req.path == "/api/models/ggml-org/tiny-llamas" {
                Resp::json(json!({"id": "ggml-org/tiny-llamas", "tags": ["gguf", "license:apache-2.0"], "gated": false, "lastModified": "2024-05-22T13:15:20.000Z",
                    "gguf": {"total": 24407712, "architecture": "llama", "context_length": 128}}))
            } else if req.path.starts_with("/api/models/ggml-org/tiny-llamas/tree/main") {
                Resp::json(json!([
                    {"type": "file", "oid": "81fb", "size": 1738, "path": ".gitattributes"},
                    {"type": "directory", "oid": "aa", "size": 0, "path": "sub"},
                    {"type": "file", "oid": "da91", "size": 19077344, "lfs": {"oid": "6151b1929d7f5aa3385d9ddef3393e55587c0a55de661562322bc51dfda93a04", "size": 19077344, "pointerSize": 133}, "path": "stories15M-q4_0.gguf"},
                    {"type": "file", "oid": "3e86", "size": 1185376, "lfs": {"oid": "047bf46455a544931cff6fef14d7910154c56afbc23ab1c5e56a72e69912c04b", "size": 1185376, "pointerSize": 132}, "path": "sub/stories 260K.gguf"}
                ]))
            } else {
                Resp::error(404, json!({"error": "Repository not found"}))
            }
        }).await;
        let state = AppState::for_tests();
        let found = hf_search(&state, &server.url, " tiny llamas ").await.unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].id.as_str(), found[0].downloads, found[0].license.as_deref(), found[0].gated),
            ("ggml-org/tiny-llamas", Some(25300), None, false)
        );
        assert_eq!((found[1].license.as_deref(), found[1].gated), (Some("mit"), true));
        let sent = server.last("/api/models?").path;
        assert!(sent.contains("search=tiny+llamas") || sent.contains("search=tiny%20llamas"), "{sent}");
        assert!(sent.contains("filter=gguf"));

        let repo = hf_files(&state, &server.url, "ggml-org/tiny-llamas").await.unwrap();
        assert_eq!(
            (repo.license.as_deref(), repo.architecture.as_deref(), repo.context_length),
            (Some("apache-2.0"), Some("llama"), Some(128))
        );
        assert_eq!(repo.files.len(), 2, "only GGUF files are listed");
        assert_eq!(repo.files[0].path, "sub/stories 260K.gguf", "smallest first");
        assert_eq!(repo.files[0].sha256.as_deref(), Some("047bf46455a544931cff6fef14d7910154c56afbc23ab1c5e56a72e69912c04b"));
        assert_eq!(repo.files[0].url, format!("{}/ggml-org/tiny-llamas/resolve/main/sub/stories%20260K.gguf", server.url));
        assert_eq!(repo.files[1].size_bytes, Some(19077344));

        assert!(hf_files(&state, &server.url, "missing/repo").await.unwrap_err().to_string().contains("no repository"));
        assert!(hf_files(&state, &server.url, "../../etc").await.is_err());
        assert!(hf_search(&state, &server.url, "  ").await.is_err());
        assert!(hf_search(&state, "http://127.0.0.1:9", "x").await.unwrap_err().to_string().contains("Could not reach Hugging Face"));
        let _ = fs::remove_dir_all(&state.data_dir);
    }
}
