//! Local runtimes: which are installed, which are answering, and the server processes NEXUS
//! starts itself.
//!
//! Detection only looks: it checks well-known install locations and asks default local ports
//! what is listening. Nothing is installed or downloaded here.
//!
//! A managed process is started from a runtime's own binary (found by detection, never a path
//! supplied by the UI) with a fixed argument list and no shell, always bound to 127.0.0.1.

use crate::ai::{self, Provider};
use crate::error::{AppError, Result};
use crate::{activity, db, platform, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::watch;

pub const EVENT_RUNTIME: &str = "nexus://lab-runtime";

const QUICK_PROBE: Duration = Duration::from_millis(1500);
const MAX_MANAGED: usize = 4;
const MAX_LOG_LINES: usize = 500;
/// How long a started server may take to answer before it is reported as not responding.
const READY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Serialize, Clone, Debug)]
pub struct Installed {
    pub path: String,
    pub version: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Endpoint {
    pub url: String,
    pub running: bool,
    pub version: Option<String>,
    pub models: Option<usize>,
    pub detail: Option<String>,
    /// The saved connection that uses this address, if any.
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub provider_enabled: Option<bool>,
    /// Set when NEXUS started the server at this address.
    pub managed_id: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SetupStep {
    pub title: &'static str,
    pub detail: &'static str,
    /// A command to copy. NEXUS never runs it.
    pub command: Option<&'static str>,
    pub url: Option<&'static str>,
}

#[derive(Serialize, Clone, Debug)]
pub struct RuntimeStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// The connection kind used to talk to it.
    pub kind: &'static str,
    pub installed: Option<Installed>,
    pub endpoints: Vec<Endpoint>,
    /// NEXUS can start and stop a server process for this runtime on this machine.
    pub can_start: bool,
    pub setup: Vec<SetupStep>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ManagedInfo {
    pub id: String,
    /// "ollama" or "llamacpp".
    pub runtime: String,
    pub url: String,
    pub port: u16,
    /// Model file being served (llama.cpp only).
    pub model: Option<String>,
    /// "starting", "running", "unresponsive" or "exited".
    pub status: String,
    pub exit_code: Option<i64>,
    pub pid: Option<i64>,
    pub started_at: String,
    /// The exact command line, for display.
    pub command: String,
}

struct Proc {
    info: ManagedInfo,
    logs: Arc<Mutex<VecDeque<String>>>,
    stop: watch::Sender<bool>,
    /// The program that was started, to recognise it again after an unclean exit.
    binary: PathBuf,
}

/// File in the app's data folder listing the servers that are running, so a server left behind
/// by a crash or a forced kill can be stopped on the next launch.
const RECORDS_FILE: &str = "lab-servers.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerRecord {
    pub pid: u32,
    pub binary: String,
    pub started_at: String,
}

fn save_records(state: &AppState) {
    let records: Vec<ServerRecord> = state
        .lab
        .runtimes
        .lock()
        .values()
        .filter(|p| p.info.status != "exited")
        .filter_map(|p| {
            Some(ServerRecord {
                pid: u32::try_from(p.info.pid?).ok()?,
                binary: p.binary.to_string_lossy().into_owned(),
                started_at: p.info.started_at.clone(),
            })
        })
        .collect();
    let path = state.data_dir.join(RECORDS_FILE);
    if records.is_empty() {
        let _ = std::fs::remove_file(path);
    } else if let Ok(text) = serde_json::to_string(&records) {
        let _ = std::fs::write(path, text);
    }
}

/// Processes of a recorded server that are still running: members of its process group (Unix)
/// or the process itself (Windows) whose command line still names the recorded program.
/// Anything else, such as an unrelated process that reused the number, is left alone.
#[cfg(unix)]
fn leftover_members(record: &ServerRecord, ps_output: &str) -> Vec<i32> {
    let name = Path::new(&record.binary).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.is_empty() {
        return vec![];
    }
    ps_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid: i32 = parts.next()?.parse().ok()?;
            let pgid: u32 = parts.next()?.parse().ok()?;
            let command: Vec<&str> = parts.collect();
            let program = Path::new(command.first()?).file_name()?.to_string_lossy().into_owned();
            (pgid == record.pid && program == name).then_some(pid)
        })
        .collect()
}

/// Called once at startup: stop servers a previous run started and could not stop itself.
pub fn reap_leftovers(data_dir: &Path) {
    let path = data_dir.join(RECORDS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let records: Vec<ServerRecord> = serde_json::from_str(&text).unwrap_or_default();
    if records.is_empty() {
        return;
    }
    #[cfg(unix)]
    {
        let Ok(out) = std::process::Command::new("ps").args(["-axo", "pid=,pgid=,command="]).output() else { return };
        let listing = String::from_utf8_lossy(&out.stdout).into_owned();
        for record in &records {
            for pid in leftover_members(record, &listing) {
                // SAFETY: plain signal delivery to a process this app started in an earlier run.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }
    #[cfg(windows)]
    for record in &records {
        let name = Path::new(&record.binary).file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let mut cmd = std::process::Command::new("tasklist");
        cmd.args(["/FI", &format!("PID eq {}", record.pid), "/NH", "/FO", "CSV"]);
        platform::process::hide_window(&mut cmd);
        let still_ours =
            cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&format!("\"{name}\""))).unwrap_or(false);
        if still_ours {
            platform::process::kill_tree(Some(record.pid));
        }
    }
}

/// Server processes started by NEXUS.
#[derive(Default)]
pub struct Managed {
    procs: Mutex<HashMap<String, Proc>>,
}

impl Managed {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Proc>> {
        self.procs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn list(&self) -> Vec<ManagedInfo> {
        let mut all: Vec<ManagedInfo> = self.lock().values().map(|p| p.info.clone()).collect();
        all.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        all
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut ManagedInfo)) {
        if let Some(p) = self.lock().get_mut(id) {
            change(&mut p.info);
        }
    }

    /// Stop every managed process now. Used when the app exits.
    pub fn stop_all(&self) {
        for p in self.lock().values() {
            let _ = p.stop.send(true);
            if p.info.status != "exited" {
                platform::process::kill_tree(p.info.pid.and_then(|pid| u32::try_from(pid).ok()));
            }
        }
    }
}

// ------------------------------------------------------------------ detection

/// Folders searched for runtime binaries: the user's command search path.
fn search_dirs() -> Vec<PathBuf> {
    platform::programs::search_dirs()
}

/// First executable called `name` in `dirs`, then in `extra` (full paths).
pub(crate) fn find_binary(name: &str, dirs: &[PathBuf], extra: &[PathBuf]) -> Option<PathBuf> {
    platform::programs::find_in(name, dirs, extra)
}

// Which entries are pushed depends on the platform, so this cannot be a `vec![...]`.
#[allow(clippy::vec_init_then_push, unused_mut)]
fn ollama_binary() -> Option<PathBuf> {
    let mut extra = vec![];
    #[cfg(target_os = "macos")]
    extra.push(PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"));
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        extra.push(PathBuf::from(local).join("Programs").join("Ollama").join("ollama.exe"));
    }
    find_binary("ollama", &search_dirs(), &extra)
}

fn llama_server_binary() -> Option<PathBuf> {
    find_binary("llama-server", &search_dirs(), &[])
}

fn lmstudio_binary() -> Option<PathBuf> {
    let extra: Vec<PathBuf> = platform::paths::home()
        .map(|h| h.join(".lmstudio").join("bin").join(if cfg!(windows) { "lms.exe" } else { "lms" }))
        .into_iter()
        .collect();
    find_binary("lms", &search_dirs(), &extra)
}

/// Pull a version number out of what a `--version` flag printed.
pub(crate) fn parse_version(output: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?i)version[^\d\n]{0,12}(\d+(?:\.\d+)*[\w.+-]*)").unwrap());
    re.captures(output).map(|c| c[1].trim_end_matches(['.', ',']).to_string())
}

/// Ask a runtime's own binary for its version. Fixed arguments, short timeout, no shell.
async fn binary_version(path: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    cmd.env_remove("NEXUS_E2E_PORT");
    let out = tokio::time::timeout(Duration::from_secs(4), cmd.output()).await.ok()?.ok()?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    parse_version(&text)
}

fn same_endpoint(a: &str, b: &str) -> bool {
    let norm = |u: &str| {
        let u = u.trim_end_matches('/').to_ascii_lowercase();
        u.replace("://localhost", "://127.0.0.1").replace("://[::1]", "://127.0.0.1")
    };
    norm(a) == norm(b)
}

async fn get_json(state: &AppState, provider: Option<&Provider>, url: String) -> Option<(u16, Value)> {
    let req = state.http.get(url).timeout(QUICK_PROBE);
    let req = match provider {
        Some(p) => ai::authed(p, req).ok()?.req,
        None => req,
    };
    let resp = req.send().await.ok()?;
    let status = resp.status().as_u16();
    Some((status, serde_json::from_str(&ai::read_body_prefix(resp, ai::MAX_JSON_BYTES).await).unwrap_or(Value::Null)))
}

/// Ask an address whether the expected kind of server is answering there.
async fn probe(state: &AppState, kind: &str, url: &str, provider: Option<&Provider>) -> Endpoint {
    let mut ep = Endpoint { url: url.to_string(), ..Default::default() };
    match kind {
        "ollama" => {
            let Some((200, v)) = get_json(state, provider, format!("{url}/api/version")).await else { return ep };
            let Some(version) = v.get("version").and_then(|x| x.as_str()) else { return ep };
            ep.running = true;
            ep.version = Some(version.to_string());
            if let Some((200, tags)) = get_json(state, provider, format!("{url}/api/tags")).await {
                ep.models = tags.get("models").and_then(|m| m.as_array()).map(|m| m.len());
            }
        }
        "llamacpp" => {
            let Some((status, v)) = get_json(state, provider, format!("{url}/health")).await else { return ep };
            // Something else may be using the port: only a llama.cpp-shaped answer counts.
            let ready = status == 200 && v.get("status").and_then(|s| s.as_str()) == Some("ok");
            let loading = status == 503 && v.pointer("/error/type").and_then(|t| t.as_str()) == Some("unavailable_error");
            if !(ready || loading) {
                return ep;
            }
            ep.running = true;
            ep.detail = loading.then(|| "Loading its model".to_string());
            if let Some((200, list)) = get_json(state, provider, format!("{url}/v1/models")).await {
                let data = list.get("data").and_then(|d| d.as_array());
                ep.models = data.map(|d| d.len());
                if let Some(id) = data.and_then(|d| d.first()).and_then(|m| m.get("id")).and_then(|i| i.as_str()) {
                    ep.detail = Some(format!("Serving {id}"));
                }
            }
        }
        _ => {
            let Some((200, list)) = get_json(state, provider, format!("{url}/models")).await else { return ep };
            let Some(data) = list.get("data").and_then(|d| d.as_array()) else { return ep };
            ep.running = true;
            ep.models = Some(data.len());
        }
    }
    ep
}

const OLLAMA_SETUP: &[SetupStep] = &[
    SetupStep {
        title: "Install Ollama",
        detail: "Download the installer for macOS or Windows. On Linux, or with Homebrew on macOS, use the command below.",
        command: if cfg!(target_os = "linux") {
            Some("curl -fsSL https://ollama.com/install.sh | sh")
        } else if cfg!(target_os = "macos") {
            Some("brew install ollama")
        } else {
            None
        },
        url: Some("https://ollama.com/download"),
    },
    SetupStep {
        title: "Start the server",
        detail: "The desktop app starts it for you. Without the app, run this in a terminal, or start it from this screen once Ollama is installed.",
        command: Some("ollama serve"),
        url: None,
    },
    SetupStep {
        title: "Get a model",
        detail: "Pull a model by name from Model Lab → Library → Add model. Models can be several gigabytes; nothing is downloaded until you ask.",
        command: None,
        url: Some("https://ollama.com/library"),
    },
];

const LLAMACPP_SETUP: &[SetupStep] = &[
    SetupStep {
        title: "Install llama.cpp",
        detail: "Install it with Homebrew on macOS or Linux, or download a build for your system from the releases page.",
        command: if cfg!(windows) { None } else { Some("brew install llama.cpp") },
        url: Some("https://github.com/ggml-org/llama.cpp/releases"),
    },
    SetupStep {
        title: "Get a GGUF model file",
        detail: "Download one in Model Lab → Library → Add model, or put a .gguf file you already have in the model folder.",
        command: None,
        url: None,
    },
    SetupStep {
        title: "Start the server",
        detail: "Start it from this screen once llama-server is installed and a model file is in the model folder, or run it yourself:",
        command: Some("llama-server -m /path/to/model.gguf --host 127.0.0.1 --port 8080"),
        url: None,
    },
];

const OPENAI_SETUP: &[SetupStep] = &[
    SetupStep {
        title: "Run any OpenAI-compatible server",
        detail: "LM Studio, vLLM, LocalAI and many others expose the OpenAI chat API on a local port. LM Studio uses port 1234 by default.",
        command: None,
        url: Some("https://lmstudio.ai"),
    },
    SetupStep {
        title: "Start LM Studio's local server",
        detail: "In LM Studio, open the Developer tab and start the server, or use its command-line tool:",
        command: Some("lms server start"),
        url: None,
    },
    SetupStep {
        title: "Add it as a connection",
        detail: "Add a connection of type OpenAI-compatible with the server's address, for example http://localhost:1234/v1.",
        command: None,
        url: None,
    },
];

/// Look for runtimes on this machine and ask the known addresses what is listening.
pub async fn detect(state: &AppState) -> Result<Vec<RuntimeStatus>> {
    let providers = ai::all_providers(state)?;
    let managed = state.lab.runtimes.list();
    #[allow(clippy::type_complexity)] // a local table of rows, read once below
    let groups: [(&'static str, &'static str, &'static str, Option<PathBuf>, &[&str], &'static [SetupStep], bool); 3] = [
        ("ollama", "Ollama", "ollama", ollama_binary(), &["http://localhost:11434"], OLLAMA_SETUP, true),
        // 8080 is llama-server's long-standing default port; newer builds document 9931.
        (
            "llamacpp",
            "llama.cpp server",
            "llamacpp",
            llama_server_binary(),
            &["http://localhost:8080", "http://localhost:9931"],
            LLAMACPP_SETUP,
            true,
        ),
        (
            "openai",
            "OpenAI-compatible (LM Studio and others)",
            "openai",
            lmstudio_binary(),
            &["http://localhost:1234/v1"],
            OPENAI_SETUP,
            false,
        ),
    ];
    let mut out = vec![];
    for (id, name, kind, binary, defaults, setup, startable) in groups {
        let installed = match &binary {
            // LM Studio's tool is only checked for presence; its flags are not known to NEXUS.
            Some(path) if id == "openai" => Some(Installed { path: path.to_string_lossy().into_owned(), version: None }),
            Some(path) => Some(Installed { path: path.to_string_lossy().into_owned(), version: binary_version(path).await }),
            None => None,
        };
        // Candidate addresses: saved local connections of this kind, servers NEXUS started, then defaults.
        let mut urls: Vec<String> = vec![];
        let mut add = |url: &str| {
            if !urls.iter().any(|u| same_endpoint(u, url)) {
                urls.push(url.to_string());
            }
        };
        for p in providers.iter().filter(|p| p.kind == kind && p.local) {
            add(&p.base_url);
        }
        for m in managed.iter().filter(|m| m.runtime == id && m.status != "exited") {
            add(&m.url);
        }
        for d in defaults {
            add(d);
        }
        let mut endpoints = vec![];
        for (i, url) in urls.iter().enumerate() {
            let provider = providers.iter().find(|p| p.kind == kind && same_endpoint(&p.base_url, url));
            let known = provider.is_some() || managed.iter().any(|m| m.runtime == id && same_endpoint(&m.url, url));
            // A disabled connection is never contacted.
            let mut ep = match provider {
                Some(p) if !p.enabled => Endpoint { url: url.clone(), detail: Some("Connection disabled".into()), ..Default::default() },
                _ => probe(state, kind, url, provider).await,
            };
            ep.provider_id = provider.map(|p| p.id.clone());
            ep.provider_name = provider.map(|p| p.name.clone());
            ep.provider_enabled = provider.map(|p| p.enabled);
            ep.managed_id =
                managed.iter().find(|m| m.runtime == id && m.status != "exited" && same_endpoint(&m.url, url)).map(|m| m.id.clone());
            // Show default addresses only when something answers there, plus one as a hint.
            if ep.running || known || (i == urls.len() - defaults.len() && !endpoints.iter().any(|e: &Endpoint| e.running)) {
                endpoints.push(ep);
            }
        }
        out.push(RuntimeStatus {
            id,
            name,
            kind,
            can_start: startable && installed.is_some(),
            installed,
            endpoints,
            setup: setup.to_vec(),
        });
    }
    Ok(out)
}

// ------------------------------------------------------------------ managed processes

#[derive(Deserialize, Debug)]
pub struct StartSpec {
    /// "ollama" or "llamacpp".
    pub runtime: String,
    pub port: u16,
    /// llama.cpp: name of a model file in the model folder.
    pub model_file: Option<String>,
    /// llama.cpp: context window in tokens (omit to use the model's own).
    pub context: Option<u32>,
}

fn spawn_log_reader<S: AsyncRead + Unpin + Send + 'static>(stream: S, logs: Arc<Mutex<VecDeque<String>>>) {
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line: String = line.chars().take(2000).collect();
            let mut logs = logs.lock().unwrap_or_else(|e| e.into_inner());
            if logs.len() >= MAX_LOG_LINES {
                logs.pop_front();
            }
            logs.push_back(line);
        }
    });
}

/// Start `binary` as a managed server. `binary` comes from detection (or a test), never the UI.
pub(crate) async fn start_process<R: Runtime>(app: &AppHandle<R>, spec: &StartSpec, binary: &Path) -> Result<ManagedInfo> {
    let state = app.state::<AppState>();
    if spec.port < 1024 {
        return Err("Choose a port between 1024 and 65535".into());
    }
    if state.lab.runtimes.list().iter().filter(|m| m.status != "exited").count() >= MAX_MANAGED {
        return Err(AppError::Msg(format!("{MAX_MANAGED} servers are already running. Stop one first.")));
    }
    // The port must be free; binding it briefly is the only reliable check.
    std::net::TcpListener::bind(("127.0.0.1", spec.port)).map_err(|_| {
        AppError::Msg(format!("Port {} is already in use. Pick another port, or connect to what is running there.", spec.port))
    })?;

    let mut args: Vec<String> = vec![];
    let mut model = None;
    let mut cmd = tokio::process::Command::new(binary);
    // The server needs neither this app's test hook nor any credentials from the environment;
    // the Trusted policy removes both while keeping its own settings (OLLAMA_MODELS, …). This is
    // set first so the server's own variables below are added on top.
    cmd.env_clear().envs(crate::exec::env::build(crate::exec::Level::Trusted, &[]));
    match spec.runtime.as_str() {
        "ollama" => {
            args.push("serve".into());
            cmd.env("OLLAMA_HOST", format!("127.0.0.1:{}", spec.port));
        }
        "llamacpp" => {
            let name = super::storage::safe_name(spec.model_file.as_deref().ok_or("Choose a model file for llama-server to serve")?)?;
            let path = super::storage::dir(&state)?.join(&name);
            if !std::fs::symlink_metadata(&path).map(|m| m.file_type().is_file()).unwrap_or(false) {
                return Err("That model file is not in the model folder".into());
            }
            args.extend([
                "-m".into(),
                path.to_string_lossy().into_owned(),
                "--host".into(),
                "127.0.0.1".into(),
                "--port".into(),
                spec.port.to_string(),
            ]);
            if let Some(ctx) = spec.context {
                if !(256..=1_048_576).contains(&ctx) {
                    return Err("Context size must be between 256 and 1048576 tokens".into());
                }
                args.extend(["-c".into(), ctx.to_string()]);
            }
            model = Some(name);
        }
        _ => return Err("NEXUS can only start Ollama and llama.cpp servers".into()),
    }
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    platform::process::detach_tree(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| AppError::Msg(format!("Could not start {}: {e}", binary.display())))?;
    let pid = child.id();
    let logs = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(out) = child.stdout.take() {
        spawn_log_reader(out, logs.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_log_reader(err, logs.clone());
    }
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let id = db::new_id();
    let info = ManagedInfo {
        id: id.clone(),
        runtime: spec.runtime.clone(),
        url: format!("http://127.0.0.1:{}", spec.port),
        port: spec.port,
        model,
        status: "starting".into(),
        exit_code: None,
        pid: pid.map(i64::from),
        started_at: db::now(),
        command: std::iter::once(binary.to_string_lossy().into_owned()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" "),
    };
    state.lab.runtimes.lock().insert(id.clone(), Proc { info: info.clone(), logs, stop: stop_tx, binary: binary.to_path_buf() });
    save_records(&state);
    activity::log(&state.db.conn(), "models", &format!("Started a {} server on port {}", spec.runtime, spec.port), None);

    // Waiter: ends the process on request and records how it ended.
    let (wait_app, wait_id) = (app.clone(), id.clone());
    tauri::async_runtime::spawn(async move {
        let code = tokio::select! {
            status = child.wait() => status.ok().and_then(|s| s.code()).map(i64::from),
            _ = stop_rx.changed() => {
                // Ask politely first so the server can release its model, then insist.
                platform::process::terminate_tree(pid);
                match tokio::time::timeout(Duration::from_secs(4), child.wait()).await {
                    Ok(status) => status.ok().and_then(|s| s.code()).map(i64::from),
                    Err(_) => {
                        platform::process::kill_tree(pid);
                        let _ = child.start_kill();
                        child.wait().await.ok().and_then(|s| s.code()).map(i64::from)
                    }
                }
            }
        };
        let state = wait_app.state::<AppState>();
        state.lab.runtimes.update(&wait_id, |info| {
            info.status = "exited".into();
            info.exit_code = code;
            info.pid = None;
        });
        save_records(&state);
        let _ = wait_app.emit(EVENT_RUNTIME, &wait_id);
    });

    // Readiness: the server is "running" once it answers, not merely once it has a process.
    let (ready_app, ready_id, ready_url, ready_kind) = (app.clone(), id.clone(), info.url.clone(), spec.runtime.clone());
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let state = ready_app.state::<AppState>();
            let current = state.lab.runtimes.list().into_iter().find(|m| m.id == ready_id).map(|m| m.status);
            if current.as_deref() != Some("starting") {
                return;
            }
            if probe(&state, &ready_kind, &ready_url, None).await.running {
                state.lab.runtimes.update(&ready_id, |info| info.status = "running".into());
                let _ = ready_app.emit(EVENT_RUNTIME, &ready_id);
                return;
            }
            if started.elapsed() > READY_TIMEOUT {
                state.lab.runtimes.update(&ready_id, |info| info.status = "unresponsive".into());
                let _ = ready_app.emit(EVENT_RUNTIME, &ready_id);
                return;
            }
        }
    });

    let _ = app.emit(EVENT_RUNTIME, &id);
    Ok(info)
}

// ------------------------------------------------------------------ loaded models

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct LoadedModel {
    pub name: String,
    /// Memory the loaded model occupies, as the runtime reports it.
    pub size_bytes: Option<u64>,
    /// The part of it held in GPU memory.
    pub size_vram_bytes: Option<u64>,
    pub context_length: Option<u64>,
    pub expires_at: Option<String>,
}

pub fn parse_loaded(ps: &Value) -> Vec<LoadedModel> {
    ps.get("models")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(LoadedModel {
                name: m.get("name").and_then(|n| n.as_str())?.to_string(),
                size_bytes: m.get("size").and_then(|v| v.as_u64()),
                size_vram_bytes: m.get("size_vram").and_then(|v| v.as_u64()),
                context_length: m.get("context_length").and_then(|v| v.as_u64()),
                expires_at: m.get("expires_at").and_then(|v| v.as_str()).map(String::from),
            })
        })
        .collect()
}

/// Models a runtime currently holds in memory. Only Ollama reports this.
pub async fn loaded(state: &AppState, provider: &Provider) -> Result<Vec<LoadedModel>> {
    if provider.kind != "ollama" {
        return Ok(vec![]);
    }
    let request = ai::authed(provider, state.http.get(format!("{}/api/ps", provider.base_url)))?;
    Ok(parse_loaded(&ai::send_json_within(request, ai::PROBE_TIMEOUT).await?))
}

/// Ask Ollama to release a model from memory now instead of after its idle timeout.
pub async fn unload(state: &AppState, provider: &Provider, model: &str) -> Result<()> {
    if provider.kind != "ollama" {
        return Err("Only Ollama can be asked to unload a model".into());
    }
    let body = json!({ "model": model, "keep_alive": 0 });
    let request = ai::authed(provider, state.http.post(format!("{}/api/generate", provider.base_url)).json(&body))?;
    ai::send_json_within(request, Duration::from_secs(30)).await?;
    Ok(())
}

// ------------------------------------------------------------------ commands

#[derive(Serialize)]
pub struct RuntimeReport {
    pub runtimes: Vec<RuntimeStatus>,
    pub managed: Vec<ManagedInfo>,
}

#[tauri::command]
pub async fn lab_runtimes(state: State<'_, AppState>) -> Result<RuntimeReport> {
    Ok(RuntimeReport { runtimes: detect(&state).await?, managed: state.lab.runtimes.list() })
}

#[tauri::command]
pub async fn lab_runtime_start<R: Runtime>(app: AppHandle<R>, spec: StartSpec) -> Result<ManagedInfo> {
    let binary = match spec.runtime.as_str() {
        "ollama" => ollama_binary().ok_or("Ollama is not installed on this machine")?,
        "llamacpp" => llama_server_binary().ok_or("llama-server is not installed on this machine")?,
        _ => return Err("NEXUS can only start Ollama and llama.cpp servers".into()),
    };
    start_process(&app, &spec, &binary).await
}

#[tauri::command]
pub fn lab_runtime_stop(state: State<'_, AppState>, id: String) -> Result<()> {
    let procs = state.lab.runtimes.lock();
    let proc = procs.get(&id).ok_or("That server is not managed by NEXUS")?;
    let _ = proc.stop.send(true);
    Ok(())
}

/// Recent output of a managed server.
#[tauri::command]
pub fn lab_runtime_logs(state: State<'_, AppState>, id: String) -> Result<String> {
    let procs = state.lab.runtimes.lock();
    let proc = procs.get(&id).ok_or("That server is not managed by NEXUS")?;
    let logs = proc.logs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(logs.iter().cloned().collect::<Vec<_>>().join("\n"))
}

/// Forget a managed server that has exited.
#[tauri::command]
pub fn lab_runtime_dismiss(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut procs = state.lab.runtimes.lock();
    match procs.get(&id) {
        Some(p) if p.info.status == "exited" => {
            procs.remove(&id);
            Ok(())
        }
        Some(_) => Err("Stop the server before removing it from the list".into()),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn lab_runtime_loaded(state: State<'_, AppState>, provider_id: String) -> Result<Vec<LoadedModel>> {
    let p = ai::usable_provider(&state, &provider_id)?;
    loaded(&state, &p).await
}

#[tauri::command]
pub async fn lab_runtime_unload(state: State<'_, AppState>, provider_id: String, model: String) -> Result<()> {
    let p = ai::usable_provider(&state, &provider_id)?;
    unload(&state, &p, &model).await
}

#[cfg(test)]
mod tests {
    use super::super::testserver::{serve, Resp};
    use super::*;
    use rusqlite::params;

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

    #[test]
    fn version_numbers_are_read_from_real_tool_output() {
        assert_eq!(parse_version("ollama version is 0.40.16").as_deref(), Some("0.40.16"));
        assert_eq!(
            parse_version("Warning: could not connect to a running Ollama instance\nWarning: client version is 0.5.7\n").as_deref(),
            Some("0.5.7")
        );
        assert_eq!(parse_version("version: 4589 (a1b2c3d)\nbuilt with Apple clang").as_deref(), Some("4589"));
        assert_eq!(parse_version("no numbers here"), None);
    }

    #[test]
    fn local_addresses_compare_equal_however_they_are_written() {
        assert!(same_endpoint("http://localhost:11434", "http://127.0.0.1:11434/"));
        assert!(same_endpoint("HTTP://LocalHost:1234/v1", "http://127.0.0.1:1234/v1"));
        assert!(!same_endpoint("http://localhost:11434", "http://localhost:11435"));
    }

    #[cfg(unix)]
    #[test]
    fn binaries_are_found_only_when_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("nexus-bin-{}", db::new_id()));
        std::fs::create_dir_all(dir.join("second")).unwrap();
        std::fs::write(dir.join("ollama"), "not executable").unwrap();
        std::fs::write(dir.join("second/ollama"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(dir.join("second/ollama"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let dirs = [dir.clone(), dir.join("second")];
        assert_eq!(find_binary("ollama", &dirs, &[]), Some(dir.join("second/ollama")));
        assert_eq!(find_binary("llama-server", &dirs, &[]), None);
        assert_eq!(
            find_binary("llama-server", &dirs, &[dir.join("second/ollama")]),
            Some(dir.join("second/ollama")),
            "known install locations are checked after the PATH"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn probes_recognise_each_runtime_and_ignore_other_servers() {
        let ollama = serve(|req| match req.path.as_str() {
            "/api/version" => Resp::json(json!({"version": "0.40.16"})),
            "/api/tags" => Resp::json(json!({"models": [{"name": "a"}, {"name": "b"}]})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let llama = serve(|req| match req.path.as_str() {
            "/health" => Resp::json(json!({"status": "ok"})),
            "/v1/models" => Resp::json(json!({"data": [{"id": "tiny.gguf"}]})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let loading = serve(|req| match req.path.as_str() {
            "/health" => Resp::error(503, json!({"error": {"code": 503, "message": "Loading model", "type": "unavailable_error"}})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let website = serve(|_| Resp::bytes(200, "text/html", b"<html>hello</html>".to_vec())).await;
        let state = AppState::for_tests();

        let o = probe(&state, "ollama", &ollama.url, None).await;
        assert_eq!((o.running, o.version.as_deref(), o.models), (true, Some("0.40.16"), Some(2)));
        let l = probe(&state, "llamacpp", &llama.url, None).await;
        assert_eq!((l.running, l.models, l.detail.as_deref()), (true, Some(1), Some("Serving tiny.gguf")));
        let busy = probe(&state, "llamacpp", &loading.url, None).await;
        assert_eq!((busy.running, busy.detail.as_deref()), (true, Some("Loading its model")));
        let generic = probe(&state, "openai", &format!("{}/v1", llama.url), None).await;
        assert_eq!((generic.running, generic.models), (true, Some(1)));

        // A different kind of server on the port is not mistaken for a runtime.
        for kind in ["ollama", "llamacpp", "openai"] {
            assert!(!probe(&state, kind, &website.url, None).await.running, "{kind}");
            assert!(!probe(&state, kind, "http://127.0.0.1:9", None).await.running, "{kind}");
        }
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn detection_reports_saved_connections_and_never_contacts_disabled_ones() {
        let server = serve(|req| match req.path.as_str() {
            "/api/version" => Resp::json(json!({"version": "1.2.3"})),
            "/api/tags" => Resp::json(json!({"models": []})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "mine", "ollama", &server.url);
        let report = detect(&state).await.unwrap();
        let ollama = report.iter().find(|r| r.id == "ollama").unwrap();
        let ep = ollama.endpoints.iter().find(|e| e.provider_id.as_deref() == Some("mine")).unwrap();
        assert_eq!((ep.running, ep.version.as_deref(), ep.models, ep.provider_enabled), (true, Some("1.2.3"), Some(0), Some(true)));
        assert_eq!(ollama.setup.len(), 3);
        assert!(report.iter().all(|r| !r.setup.is_empty()));

        let before = server.requests.lock().unwrap().len();
        state.db.conn().execute("UPDATE providers SET enabled = 0 WHERE id = 'mine'", []).unwrap();
        let report = detect(&state).await.unwrap();
        let ep = report
            .iter()
            .find(|r| r.id == "ollama")
            .unwrap()
            .endpoints
            .iter()
            .find(|e| e.provider_id.as_deref() == Some("mine"))
            .unwrap()
            .clone();
        assert_eq!((ep.running, ep.detail.as_deref(), ep.provider_enabled), (false, Some("Connection disabled"), Some(false)));
        assert_eq!(server.requests.lock().unwrap().len(), before, "a disabled connection is not contacted");
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[tokio::test]
    async fn loaded_models_report_memory_and_unload_asks_ollama_to_release() {
        // Reply captured from a real Ollama 0.40 (trimmed).
        let server = serve(|req| match req.path.as_str() {
            "/api/ps" => {
                Resp::json(json!({"models": [{"name": "qwen3.8:27b", "model": "qwen3.8:27b", "size": 18490543832u64, "digest": "4795",
                "expires_at": "2026-11-15T23:08:09+05:30", "size_vram": 18490543832u64, "context_length": 4096, "runner": "mlx"}]}))
            }
            "/api/generate" => Resp::json(json!({"model": "qwen3.8:27b", "done": true, "done_reason": "unload"})),
            _ => Resp::error(404, json!({})),
        })
        .await;
        let state = AppState::for_tests();
        add_provider(&state, "p", "ollama", &server.url);
        add_provider(&state, "other", "openai", &format!("{}/v1", server.url));
        let p = ai::get_provider(&state, "p").unwrap();
        let models = loaded(&state, &p).await.unwrap();
        assert_eq!(
            models,
            [LoadedModel {
                name: "qwen3.8:27b".into(),
                size_bytes: Some(18490543832),
                size_vram_bytes: Some(18490543832),
                context_length: Some(4096),
                expires_at: Some("2026-11-15T23:08:09+05:30".into())
            }]
        );
        unload(&state, &p, "qwen3.8:27b").await.unwrap();
        let sent = server.last("/api/generate").json();
        assert_eq!((sent["model"].as_str(), sent["keep_alive"].as_i64()), (Some("qwen3.8:27b"), Some(0)));
        // Other kinds report nothing rather than guessing.
        let other = ai::get_provider(&state, "other").unwrap();
        assert!(loaded(&state, &other).await.unwrap().is_empty());
        assert!(unload(&state, &other, "x").await.is_err());
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }
}

#[cfg(all(test, unix))]
mod leftover_tests {
    use super::*;

    #[test]
    fn only_processes_of_the_recorded_program_and_group_are_selected() {
        let record = ServerRecord { pid: 500, binary: "/usr/local/bin/ollama".into(), started_at: String::new() };
        let ps = "  500   500 /usr/local/bin/ollama serve\n  612   500 /Applications/Ollama.app/Contents/Resources/ollama runner --model m --port 57420\n  700   500 /usr/bin/python3 other.py\n  800   800 /usr/local/bin/ollama serve\n  900   900 /bin/zsh\n";
        assert_eq!(leftover_members(&record, ps), [500, 612], "the server and its runner, nothing outside the group or not named ollama");
        assert!(leftover_members(&ServerRecord { binary: String::new(), ..record.clone() }, ps).is_empty());
        assert!(leftover_members(&ServerRecord { pid: 123, ..record }, ps).is_empty(), "a group that no longer exists selects nothing");
    }

    #[test]
    fn a_left_behind_server_is_stopped_at_the_next_launch() {
        use std::os::unix::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("nexus-reap-{}", db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A stand-in server in its own process group, as NEXUS starts them.
        let mut child = std::process::Command::new("/bin/sleep").arg("300").process_group(0).spawn().unwrap();
        let pid = child.id();
        // An unrelated process that must survive even though a record names its number.
        let mut other = std::process::Command::new("/bin/sleep").arg("301").spawn().unwrap();
        let records = vec![
            ServerRecord { pid, binary: "/bin/sleep".into(), started_at: String::new() },
            ServerRecord { pid: other.id(), binary: "/usr/local/bin/ollama".into(), started_at: String::new() },
        ];
        std::fs::write(dir.join(RECORDS_FILE), serde_json::to_string(&records).unwrap()).unwrap();
        reap_leftovers(&dir);
        let status = child.wait().unwrap();
        assert!(!status.success(), "the recorded server was stopped");
        assert!(other.try_wait().unwrap().is_none(), "a process that does not match its record is left alone");
        assert!(!dir.join(RECORDS_FILE).exists());
        other.kill().unwrap();
        let _ = other.wait();
        reap_leftovers(&dir); // nothing recorded: nothing happens
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// A real child process started and stopped through the managed-process code, on a mock app.
#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    fn app() -> tauri::App<MockRuntime> {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        app.manage(AppState::for_tests());
        app
    }

    /// A stand-in server binary: prints where it was told to listen, then idles.
    fn fake_binary(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            "#!/bin/sh\necho \"host=$OLLAMA_HOST args=$*\"\necho \"leak=$NEXUS_E2E_PORT$MY_API_KEY\" >&2\nexec sleep 300\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    async fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
        let started = Instant::now();
        while !check() {
            assert!(started.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_managed_server_is_started_without_a_shell_logged_and_stopped() {
        let app = app();
        let state = app.state::<AppState>();
        let dir = state.data_dir.clone();
        let binary = fake_binary(&dir, "ollama");
        std::env::set_var("MY_API_KEY", "sk-should-not-leak");
        let port = free_port();
        let spec = StartSpec { runtime: "ollama".into(), port, model_file: None, context: None };
        let info = start_process(app.handle(), &spec, &binary).await.unwrap();
        std::env::remove_var("MY_API_KEY");
        assert_eq!((info.status.as_str(), info.port, info.url.as_str()), ("starting", port, format!("http://127.0.0.1:{port}").as_str()));
        assert!(info.command.ends_with("ollama serve"));
        let pid = info.pid.unwrap();

        let logs = || lab_runtime_logs(app.state::<AppState>(), info.id.clone()).unwrap();
        wait_for("the server's output", || logs().contains("leak=")).await;
        let text = logs();
        assert!(text.contains(&format!("host=127.0.0.1:{port} args=serve")), "bound to loopback with fixed arguments: {text}");
        assert!(text.contains("leak=\n") || text.ends_with("leak="), "no test hook or credential reaches the server: {text}");

        assert!(lab_runtime_dismiss(app.state::<AppState>(), info.id.clone()).is_err(), "a running server cannot be dismissed");
        lab_runtime_stop(app.state::<AppState>(), info.id.clone()).unwrap();
        wait_for("the server to exit", || state.lab.runtimes.list()[0].status == "exited").await;
        // SAFETY: signal 0 only checks whether the process exists.
        wait_for("the process to be gone", || unsafe { libc::kill(pid as libc::pid_t, 0) } != 0).await;
        lab_runtime_dismiss(app.state::<AppState>(), info.id.clone()).unwrap();
        assert!(state.lab.runtimes.list().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn starting_a_server_validates_port_runtime_and_model_file() {
        let app = app();
        let state = app.state::<AppState>();
        let dir = state.data_dir.clone();
        let binary = fake_binary(&dir, "llama-server");
        let start = |spec: StartSpec| {
            let (app, binary) = (app.handle().clone(), binary.clone());
            async move { start_process(&app, &spec, &binary).await }
        };

        let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = busy.local_addr().unwrap().port();
        let err =
            start(StartSpec { runtime: "ollama".into(), port: taken, model_file: None, context: None }).await.unwrap_err().to_string();
        assert!(err.contains("already in use"), "{err}");
        assert!(start(StartSpec { runtime: "ollama".into(), port: 80, model_file: None, context: None }).await.is_err());
        assert!(start(StartSpec { runtime: "bash".into(), port: free_port(), model_file: None, context: None }).await.is_err());

        // llama.cpp needs a model file that really is in the model folder.
        let models = crate::lab::storage::dir(&state).unwrap();
        for bad in [None, Some("../../etc/passwd".to_string()), Some("missing.gguf".to_string()), Some("/abs/path.gguf".to_string())] {
            assert!(
                start(StartSpec { runtime: "llamacpp".into(), port: free_port(), model_file: bad.clone(), context: None }).await.is_err(),
                "{bad:?}"
            );
        }
        std::fs::write(models.join("tiny.gguf"), crate::lab::gguf::testfile::sample()).unwrap();
        assert!(start(StartSpec { runtime: "llamacpp".into(), port: free_port(), model_file: Some("tiny.gguf".into()), context: Some(8) })
            .await
            .is_err());
        assert!(state.lab.runtimes.list().is_empty(), "nothing was started by the refused requests");

        let port = free_port();
        let info =
            start(StartSpec { runtime: "llamacpp".into(), port, model_file: Some("tiny.gguf".into()), context: Some(4096) }).await.unwrap();
        assert_eq!(info.model.as_deref(), Some("tiny.gguf"));
        let expected = format!("-m {} --host 127.0.0.1 --port {port} -c 4096", models.join("tiny.gguf").display());
        assert!(info.command.ends_with(&expected), "{}", info.command);
        state.lab.runtimes.stop_all();
        wait_for("the server to exit", || state.lab.runtimes.list()[0].status == "exited").await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
