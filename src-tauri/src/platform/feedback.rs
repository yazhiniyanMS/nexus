//! Feedback: a diagnostic report the user reads before sending, and links to the public issue
//! tracker. Nothing is sent by NEXUS itself; the report is copied or saved by the user.
//!
//! The report holds only what helps reproduce a problem: versions, the operating system, which
//! tools were found, and counts. It never holds source code, terminal output, file names,
//! provider addresses, credentials or environment variables.

use crate::error::{AppError, Result};
use crate::AppState;
use serde::Serialize;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_opener::OpenerExt;

/// "owner/name" of the public repository, set when the build is made (CI passes its own).
/// Local builds have none, and then the feedback links are simply not offered.
const REPOSITORY: Option<&str> = option_env!("NEXUS_REPOSITORY");

fn repository() -> Option<&'static str> {
    REPOSITORY.filter(|r| {
        let mut parts = r.split('/');
        let ok = |p: Option<&str>| {
            p.is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        };
        ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
    })
}

/// Replace the home folder with `~` and mask anything that looks like a credential.
pub fn redact(text: &str, home: Option<&str>) -> String {
    static SECRET: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let secret = SECRET.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)\b(sk-[a-z0-9_-]{8,}|gh[pousr]_[a-z0-9]{16,}|xox[abpr]-[a-z0-9-]{10,}|AKIA[0-9A-Z]{12,}|bearer\s+[a-z0-9._~+/=-]{12,})|((?:[a-z0-9_]*(?:key|token|secret|password|passwd|credential)[a-z0-9_]*)\s*[=:]\s*)("[^"]*"|'[^']*'|\S+)"#,
        )
        .unwrap()
    });
    let mut out = text.to_string();
    if let Some(home) = home.filter(|h| h.len() > 3) {
        out = out.replace(home, "~");
    }
    secret
        .replace_all(&out, |c: &regex::Captures| match c.get(2) {
            Some(name) => format!("{}[redacted]", name.as_str()),
            None => "[redacted]".to_string(),
        })
        .into_owned()
}

#[derive(Serialize)]
pub struct FeedbackInfo {
    pub app_version: &'static str,
    /// The report, already redacted. Shown to the user, who decides whether to send it.
    pub diagnostics: String,
    /// Whether this build knows the public repository, so issue links can be opened.
    pub links_available: bool,
    pub repository: Option<&'static str>,
}

fn count(state: &AppState, sql: &str) -> i64 {
    state.db.conn().query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

pub fn diagnostics(state: &AppState) -> String {
    let info = super::info(&state.data_dir);
    let shell = std::path::Path::new(&info.shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let schema = count(state, "PRAGMA user_version");
    let kinds: Vec<String> = {
        let conn = state.db.conn();
        let mut stmt = conn.prepare("SELECT kind, COUNT(*) FROM providers GROUP BY kind ORDER BY kind").ok();
        stmt.as_mut()
            .and_then(|s| {
                s.query_map([], |r| Ok(format!("{} x{}", r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .ok()
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    };
    let hw = crate::lab::hardware::hardware();
    let text = format!(
        "NEXUS {version}\n\
         System: {os} {os_version} ({arch})\n\
         CPU: {cpu}, {cores} cores, {memory}\n\
         Shell: {shell}\n\
         Git: {git}\n\
         Secure storage: {storage}\n\
         Data folder: {data}{overridden}\n\
         Database schema: {schema}\n\
         Projects: {projects}, terminals: {terminals}, agent tasks: {agents}\n\
         AI connections: {kinds}\n",
        version = info.app_version,
        os = info.os,
        os_version = info.os_version.as_deref().unwrap_or("version unknown"),
        arch = info.arch,
        cpu = hw.cpu.as_deref().unwrap_or("unknown CPU"),
        cores = hw.cores.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
        memory = hw.memory_bytes.map(|b| format!("{} GB memory", b / 1_073_741_824)).unwrap_or_else(|| "memory unknown".into()),
        git = info.git_version.as_deref().unwrap_or("not found"),
        storage = info.secure_storage,
        data = info.data_dir,
        overridden = if info.data_dir_overridden { " (set by NEXUS_DATA_DIR)" } else { "" },
        projects = count(state, "SELECT COUNT(*) FROM projects"),
        terminals = count(state, "SELECT COUNT(*) FROM term_sessions"),
        agents = count(state, "SELECT COUNT(*) FROM agent_runs"),
        kinds = if kinds.is_empty() { "none".to_string() } else { kinds.join(", ") },
    );
    redact(&text, super::paths::home().map(|h| super::paths::display(&h)).as_deref())
}

#[tauri::command]
pub fn feedback_info(state: State<'_, AppState>) -> FeedbackInfo {
    FeedbackInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        diagnostics: diagnostics(&state),
        links_available: repository().is_some(),
        repository: repository(),
    }
}

/// The address for one kind of feedback. Only these fixed pages can be opened.
fn link(repo: &str, kind: &str) -> Option<String> {
    let base = format!("https://github.com/{repo}");
    Some(match kind {
        "bug" => format!("{base}/issues/new?template=bug_report.yml"),
        "feature" => format!("{base}/issues/new?template=feature_request.yml"),
        "install" => format!("{base}/issues/new?template=installation_problem.yml"),
        "issues" => format!("{base}/issues"),
        "docs" => format!("{base}#readme"),
        "releases" => format!("{base}/releases"),
        _ => return None,
    })
}

/// Open one of the project's public pages in the user's browser. The webview names a kind, never
/// an address, so it cannot open arbitrary links.
#[tauri::command]
pub fn open_feedback<R: Runtime>(app: AppHandle<R>, kind: String) -> Result<String> {
    let repo =
        repository().ok_or("This build was made without a repository address. Copy the report and send it to whoever gave you NEXUS.")?;
    let url = link(repo, &kind).ok_or("Unknown feedback page")?;
    app.opener().open_url(&url, None::<&str>).map_err(|e| AppError::Msg(format!("Could not open the browser: {e}")))?;
    Ok(url)
}

/// Whether the welcome screen has been seen, kept with the other settings.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Onboarding {
    done: bool,
}

#[tauri::command]
pub fn onboarding_done(state: State<'_, AppState>) -> Result<bool> {
    Ok(crate::settings::get::<Onboarding>(&state.db.conn(), "onboarding")?.done)
}

#[tauri::command]
pub fn onboarding_finish(state: State<'_, AppState>) -> Result<()> {
    crate::settings::set(&state.db.conn(), "onboarding", &Onboarding { done: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_and_the_home_folder_are_removed() {
        let raw = "path /Users/dev/code/app key sk-abc123def456ghi789 and ghp_abcdefghijklmnopqrstu\nOPENAI_API_KEY=supersecret db_password: \"hunter2\" Authorization: Bearer abcdefghijklmnop.qrst";
        let clean = redact(raw, Some("/Users/dev"));
        for leaked in ["/Users/dev", "sk-abc123", "ghp_abc", "supersecret", "hunter2", "abcdefghijklmnop.qrst"] {
            assert!(!clean.contains(leaked), "{leaked} leaked in: {clean}");
        }
        assert!(clean.contains("path ~/code/app") && clean.contains("OPENAI_API_KEY=[redacted]"), "{clean}");
        assert_eq!(redact("ordinary text: 42 tokens used", None), "ordinary text: 42 tokens used");
    }

    #[test]
    fn the_report_has_versions_and_counts_but_no_addresses_or_names() {
        let state = AppState::for_tests();
        state
            .db
            .conn()
            .execute(
                "INSERT INTO projects (id, name, path, added_at) VALUES ('p', 'secret-client-project', '/work/secret-client-project', '')",
                [],
            )
            .unwrap();
        state
            .db
            .conn()
            .execute("INSERT INTO providers (id, name, kind, base_url, has_key, created_at) VALUES ('x', 'Corp gateway', 'openai', 'https://llm.internal.example/v1', 1, '')", [])
            .unwrap();
        let report = diagnostics(&state);
        assert!(
            report.contains(concat!("NEXUS ", env!("CARGO_PKG_VERSION"))) && report.contains("Projects: 1") && report.contains("openai x1"),
            "{report}"
        );
        for private in ["secret-client-project", "internal.example", "Corp gateway"] {
            assert!(!report.contains(private), "{private} must not be in the report");
        }
        if let Some(home) = super::super::paths::home() {
            assert!(!report.contains(&*home.to_string_lossy()), "the home folder is shown as ~");
        }
        let _ = std::fs::remove_dir_all(&state.data_dir);
    }

    #[test]
    fn only_known_pages_have_links() {
        assert_eq!(link("o/r", "bug").unwrap(), "https://github.com/o/r/issues/new?template=bug_report.yml");
        assert!(link("o/r", "https://evil.example").is_none());
    }
}
