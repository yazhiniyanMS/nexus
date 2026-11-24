//! Terminal: interactive shells in pseudo-terminals owned by the core.
//!
//! A terminal is the user's own shell. The only way bytes reach it is `term_write`, which the
//! Terminal screen calls for keystrokes and pastes. No agent, task or AI code path writes to a
//! terminal, so nothing here bypasses the approval rules those modules enforce.
//!
//! Output is appended to a bounded file per session so it survives a window reload and can be
//! searched. Shell processes do not survive an app restart: on the next start their sessions are
//! marked "ended" and keep their scrollback.

use crate::error::{AppError, Result};
use crate::projects::{project_root, resolve_inside};
use crate::{activity, db, platform, AppState};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager as _, Runtime, State};

pub const EVENT_OUTPUT: &str = "nexus://term-output";
pub const EVENT_CHANGED: &str = "nexus://term-changed";

/// Shells running at once, across all projects.
const MAX_LIVE: usize = 16;
/// Scrollback kept on disk per session. When the file passes the limit it is cut back to `KEEP`.
const SCROLLBACK_LIMIT: u64 = 2 * 1024 * 1024;
const SCROLLBACK_KEEP: u64 = 1024 * 1024;
const MAX_WRITE_BYTES: usize = 1024 * 1024;
const MAX_LAYOUT_BYTES: usize = 64 * 1024;
const MAX_SEARCH_MATCHES: usize = 200;

#[derive(Serialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub shell: String,
    /// Project-relative start folder ("" is the project root).
    pub cwd: String,
    /// "running", "exited" (with `exit_code`, or none if the user stopped it) or "ended"
    /// (NEXUS closed while it was running).
    pub status: String,
    pub exit_code: Option<i64>,
    pub pid: Option<i64>,
    pub created_at: String,
    pub ended_at: Option<String>,
}

#[derive(Serialize)]
pub struct Scrollback {
    pub data: String,
    /// Stream position after `data`; output events at or before it are already included.
    pub end: u64,
}

#[derive(Serialize, Clone)]
struct OutputEvent {
    id: String,
    data: String,
    /// Stream position after this chunk.
    end: u64,
}

#[derive(Serialize)]
pub struct TermMatch {
    pub session_id: String,
    pub title: String,
    pub line: usize,
    pub text: String,
}

/// Bounded on-disk output log of one session.
struct Log {
    path: PathBuf,
    file: Option<File>,
    on_disk: u64,
    /// Bytes ever written, which is what output events are numbered with.
    total: u64,
}

impl Log {
    fn create(path: PathBuf) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Log { path, file: Some(file), on_disk: 0, total: 0 })
    }

    fn append(&mut self, bytes: &[u8]) -> u64 {
        self.total += bytes.len() as u64;
        if let Some(file) = &mut self.file {
            if file.write_all(bytes).is_ok() {
                self.on_disk += bytes.len() as u64;
            }
        }
        if self.on_disk > SCROLLBACK_LIMIT {
            self.trim();
        }
        self.total
    }

    /// Keep the newest `SCROLLBACK_KEEP` bytes, starting at a line boundary where there is one.
    fn trim(&mut self) {
        let Ok(mut all) = fs::read(&self.path) else { return };
        let cut = all.len().saturating_sub(SCROLLBACK_KEEP as usize);
        let start = all[cut..].iter().position(|b| *b == b'\n').map(|p| cut + p + 1).unwrap_or(cut);
        all.drain(..start);
        if fs::write(&self.path, &all).is_ok() {
            self.on_disk = all.len() as u64;
            self.file = OpenOptions::new().append(true).open(&self.path).ok();
        }
    }

    /// Stop writing; used when the session is deleted while its reader is still draining.
    fn close(&mut self) {
        self.file = None;
    }
}

struct Live {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
    log: Arc<Mutex<Log>>,
}

#[derive(Default)]
pub struct Manager {
    live: Mutex<HashMap<String, Live>>,
    /// Sessions the user stopped, so their end is not reported as the shell's own exit code.
    stopped: Mutex<std::collections::HashSet<String>>,
}

impl Manager {
    /// Ids of sessions whose shell is running.
    #[cfg(test)]
    pub fn live_ids(&self) -> Vec<String> {
        self.live.lock().unwrap().keys().cloned().collect()
    }
}

// ------------------------------------------------------------------ storage

const COLS: &str = "id, project_id, title, shell, cwd, status, exit_code, pid, created_at, ended_at";

fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        shell: r.get(3)?,
        cwd: r.get(4)?,
        status: r.get(5)?,
        exit_code: r.get(6)?,
        pid: r.get(7)?,
        created_at: r.get(8)?,
        ended_at: r.get(9)?,
    })
}

fn load(conn: &Connection, id: &str) -> Result<Session> {
    conn.query_row(&format!("SELECT {COLS} FROM term_sessions WHERE id = ?1"), [id], row_to_session)
        .optional()?
        .ok_or_else(|| AppError::Msg("Terminal session not found".into()))
}

fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("terminal")
}

fn log_path(data_dir: &Path, id: &str) -> PathBuf {
    log_dir(data_dir).join(format!("{id}.log"))
}

/// Called once at startup: shells from the previous run are gone, and scrollback files without a
/// session (for example after a project was removed) are deleted.
pub fn recover_interrupted(db: &db::Db, data_dir: &Path) -> Result<()> {
    let conn = db.conn();
    conn.execute("UPDATE term_sessions SET status = 'ended', pid = NULL, ended_at = ?1 WHERE status = 'running'", [db::now()])?;
    let known: Vec<String> = conn.prepare("SELECT id FROM term_sessions")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    if let Ok(entries) = fs::read_dir(log_dir(data_dir)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !known.iter().any(|id| name == format!("{id}.log")) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ shell

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.clamp(2, 500), rows: rows.clamp(1, 300), pixel_width: 0, pixel_height: 0 }
}

fn pty_err(e: impl std::fmt::Display) -> AppError {
    AppError::Msg(format!("Terminal error: {e}"))
}

/// Take the longest prefix of `pending` that is complete UTF-8, leaving a split character behind
/// for the next read.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(text) => {
            let out = text.to_string();
            pending.clear();
            out
        }
        Err(e) if e.error_len().is_none() => {
            let rest = pending.split_off(e.valid_up_to());
            let out = String::from_utf8_lossy(pending).into_owned();
            *pending = rest;
            out
        }
        Err(_) => {
            let out = String::from_utf8_lossy(pending).into_owned();
            pending.clear();
            out
        }
    }
}

/// Start a shell for a project. `shell` overrides the user's shell (used by tests).
pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    project_id: &str,
    cwd: Option<&str>,
    cols: u16,
    rows: u16,
    shell: Option<(String, Vec<String>)>,
) -> Result<Session> {
    let state = app.state::<AppState>();
    let root = project_root(&state, project_id)?;
    let rel = cwd.unwrap_or("").trim_matches('/').to_string();
    let dir = if rel.is_empty() { root.clone() } else { resolve_inside(&root, &rel)? };
    if !dir.is_dir() {
        return Err("That folder no longer exists in the project".into());
    }
    if state.terminal.live.lock().unwrap().len() >= MAX_LIVE {
        return Err(AppError::Msg(format!("{MAX_LIVE} terminals are already running. Close one first.")));
    }

    let (program, args) = shell.unwrap_or_else(platform::shell::interactive);
    let pair = native_pty_system().openpty(size(cols, rows)).map_err(pty_err)?;
    let mut cmd = CommandBuilder::new(&program);
    cmd.args(&args);
    cmd.cwd(&dir);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "NEXUS");
    cmd.env_remove("NEXUS_E2E_PORT");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| AppError::Msg(format!("Could not start {program}: {e}")))?;
    drop(pair.slave);
    let pid = child.process_id();
    let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
    let writer = pair.master.take_writer().map_err(pty_err)?;

    let id = db::new_id();
    fs::create_dir_all(log_dir(&state.data_dir))?;
    let log = Arc::new(Mutex::new(Log::create(log_path(&state.data_dir, &id))?));
    let shell_name = Path::new(&program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(program.clone());
    let session = {
        let conn = state.db.conn();
        conn.execute(
            "INSERT INTO term_sessions (id, project_id, title, shell, cwd, status, pid, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7)",
            params![id, project_id, shell_name, program, rel, pid.map(i64::from), db::now()],
        )?;
        let name: String = conn.query_row("SELECT name FROM projects WHERE id = ?1", [project_id], |r| r.get(0))?;
        activity::log(&conn, "terminal", &format!("Opened a terminal in {name}"), Some(project_id));
        load(&conn, &id)?
    };
    state.terminal.live.lock().unwrap().insert(id.clone(), Live { master: pair.master, writer, pid, log: log.clone() });

    // Reader: PTY output -> scrollback file and the window.
    let (reader_app, reader_id) = (app.clone(), id.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut pending = Vec::new();
        // Windows ConPTY (created with "inherit cursor") starts by asking where the cursor is and
        // prints nothing until it is told. The core answers that first question itself: the screen
        // may not be attached yet, and it ignores queries found in replayed output on purpose.
        let mut handshake = Handshake::new(cfg!(windows));
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut chunk = buf[..n].to_vec();
                    if handshake.answer(&mut chunk) {
                        let state = reader_app.state::<AppState>();
                        let mut live = state.terminal.live.lock().unwrap();
                        if let Some(session) = live.get_mut(&reader_id) {
                            let _ = session.writer.write_all(CURSOR_AT_ORIGIN).and_then(|_| session.writer.flush());
                        }
                    }
                    if chunk.is_empty() {
                        continue;
                    }
                    let end = log.lock().unwrap().append(&chunk);
                    pending.extend_from_slice(&chunk);
                    let data = take_utf8(&mut pending);
                    if !data.is_empty() {
                        let _ = reader_app.emit(EVENT_OUTPUT, OutputEvent { id: reader_id.clone(), data, end });
                    }
                }
            }
        }
    });

    // Waiter: record how the shell ended.
    let (wait_app, wait_id) = (app.clone(), id.clone());
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|s| i64::from(s.exit_code()));
        let state = wait_app.state::<AppState>();
        // A shell the user stopped was killed by a signal; it has no exit code of its own.
        let code = if state.terminal.stopped.lock().unwrap().remove(&wait_id) { None } else { code };
        // Dropping the entry closes the PTY, which hangs up anything still attached to it.
        state.terminal.live.lock().unwrap().remove(&wait_id);
        let _ = state.db.conn().execute(
            "UPDATE term_sessions SET status = 'exited', exit_code = ?2, pid = NULL, ended_at = ?3 WHERE id = ?1 AND status = 'running'",
            params![wait_id, code, db::now()],
        );
        let _ = wait_app.emit(EVENT_CHANGED, &wait_id);
    });

    let _ = app.emit(EVENT_CHANGED, &id);
    Ok(session)
}

const CURSOR_QUERY: &[u8] = b"\x1b[6n";
/// The answer to a cursor-position query for a fresh console: row 1, column 1.
const CURSOR_AT_ORIGIN: &[u8] = b"\x1b[1;1R";

/// Looks for the one cursor-position query a new Windows console sends before its first output,
/// and takes it out of the stream so the screen never answers it a second time.
struct Handshake {
    active: bool,
    seen: usize,
}

impl Handshake {
    /// Only the start of a session is searched; later queries belong to programs and go to the screen.
    const WINDOW: usize = 4096;

    fn new(active: bool) -> Self {
        Handshake { active, seen: 0 }
    }

    /// Remove the first query from `chunk`. True when it was found and must be answered.
    fn answer(&mut self, chunk: &mut Vec<u8>) -> bool {
        if !self.active {
            return false;
        }
        if let Some(at) = chunk.windows(CURSOR_QUERY.len()).position(|w| w == CURSOR_QUERY) {
            chunk.drain(at..at + CURSOR_QUERY.len());
            self.active = false;
            return true;
        }
        self.seen += chunk.len();
        if self.seen > Self::WINDOW {
            self.active = false;
        }
        false
    }
}

/// Remove ANSI escape sequences and carriage returns so output can be searched as plain text.
fn strip_ansi(text: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[PX^_][^\x1b]*\x1b\\|\x1b[ -/]*[0-~]|[\r\x07\x08]",
        )
        .unwrap()
    });
    re.replace_all(text, "").into_owned()
}

fn read_log(data_dir: &Path, id: &str) -> String {
    let mut bytes = Vec::new();
    if let Ok(mut file) = File::open(log_path(data_dir, id)) {
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        // A log is normally under the limit; this guards against reading one mid-trim.
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(SCROLLBACK_LIMIT + 8192)));
        let _ = file.read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

// ------------------------------------------------------------------ commands

/// Stop shells whose session no longer exists, which happens when their project is removed.
pub(crate) fn stop_orphans(state: &AppState) -> Result<()> {
    let running: Vec<(String, Option<u32>)> = state.terminal.live.lock().unwrap().iter().map(|(id, s)| (id.clone(), s.pid)).collect();
    for (id, pid) in running {
        let known: bool = state.db.conn().query_row("SELECT EXISTS(SELECT 1 FROM term_sessions WHERE id = ?1)", [&id], |r| r.get(0))?;
        if !known {
            if let Some(session) = state.terminal.live.lock().unwrap().get(&id) {
                session.log.lock().unwrap().close();
            }
            let _ = fs::remove_file(log_path(&state.data_dir, &id));
            platform::process::kill_session(pid);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn term_list(state: State<'_, AppState>, project_id: String) -> Result<Vec<Session>> {
    stop_orphans(&state)?;
    let conn = state.db.conn();
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM term_sessions WHERE project_id = ?1 ORDER BY created_at, rowid"))?;
    let rows = stmt.query_map([project_id], row_to_session)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[tauri::command]
pub fn term_open<R: Runtime>(
    app: AppHandle<R>,
    project_id: String,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
) -> Result<Session> {
    // Only shells this system offers may be started; the webview cannot name an arbitrary program.
    let shell = match shell.filter(|s| !s.is_empty()) {
        Some(wanted) => {
            let known =
                platform::shell::available().into_iter().find(|s| *s == wanted).ok_or("That shell is not available on this system")?;
            let args = platform::shell::interactive_args(&known);
            Some((known, args))
        }
        None => None,
    };
    open(&app, &project_id, cwd.as_deref(), cols, rows, shell)
}

/// Shells a new terminal can start on this system, the default first.
#[tauri::command]
pub fn term_shells() -> Vec<String> {
    platform::shell::available()
}

/// Send keystrokes or pasted text to a running shell.
#[tauri::command]
pub fn term_write(state: State<'_, AppState>, id: String, data: String) -> Result<()> {
    if data.len() > MAX_WRITE_BYTES {
        return Err("That is too much text to send to a terminal at once".into());
    }
    let mut live = state.terminal.live.lock().unwrap();
    let session = live.get_mut(&id).ok_or("This terminal is no longer running")?;
    session.writer.write_all(data.as_bytes())?;
    session.writer.flush()?;
    Ok(())
}

#[tauri::command]
pub fn term_resize(state: State<'_, AppState>, id: String, cols: u16, rows: u16) -> Result<()> {
    let live = state.terminal.live.lock().unwrap();
    // A shell that has just exited is not an error for a resize.
    if let Some(session) = live.get(&id) {
        session.master.resize(size(cols, rows)).map_err(pty_err)?;
    }
    Ok(())
}

/// Stop the shell and everything started from it. The session and its scrollback are kept.
#[tauri::command]
pub fn term_kill(state: State<'_, AppState>, id: String) -> Result<()> {
    let pid = state.terminal.live.lock().unwrap().get(&id).map(|s| s.pid);
    match pid {
        Some(pid) => {
            state.terminal.stopped.lock().unwrap().insert(id.clone());
            platform::process::kill_session(pid)
        }
        None => {
            load(&state.db.conn(), &id)?;
        }
    }
    Ok(())
}

/// Stop the shell if it is running and delete the session with its scrollback.
#[tauri::command]
pub fn term_close<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, id: String) -> Result<()> {
    if let Some(session) = state.terminal.live.lock().unwrap().get(&id) {
        session.log.lock().unwrap().close();
        platform::process::kill_session(session.pid);
    }
    state.db.conn().execute("DELETE FROM term_sessions WHERE id = ?1", [&id])?;
    let _ = fs::remove_file(log_path(&state.data_dir, &id));
    let _ = app.emit(EVENT_CHANGED, &id);
    Ok(())
}

#[tauri::command]
pub fn term_rename(state: State<'_, AppState>, id: String, title: String) -> Result<()> {
    let title: String = title.trim().chars().take(80).collect();
    if title.is_empty() {
        return Err("A terminal needs a name".into());
    }
    let changed = state.db.conn().execute("UPDATE term_sessions SET title = ?2 WHERE id = ?1", params![id, title])?;
    if changed == 0 {
        return Err("Terminal session not found".into());
    }
    Ok(())
}

/// Saved output of a session, with the stream position it reaches.
#[tauri::command]
pub fn term_scrollback(state: State<'_, AppState>, id: String) -> Result<Scrollback> {
    load(&state.db.conn(), &id)?;
    let log = state.terminal.live.lock().unwrap().get(&id).map(|s| s.log.clone());
    Ok(match log {
        // Hold the log while reading so `end` matches `data` exactly.
        Some(log) => {
            let guard = log.lock().unwrap();
            Scrollback { data: read_log(&state.data_dir, &id), end: guard.total }
        }
        None => Scrollback { data: read_log(&state.data_dir, &id), end: 0 },
    })
}

/// Case-insensitive search of the saved output of every terminal in a project.
#[tauri::command]
pub fn term_search(state: State<'_, AppState>, project_id: String, query: String) -> Result<Vec<TermMatch>> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(vec![]);
    }
    let sessions = term_list(state.clone(), project_id)?;
    let mut matches = vec![];
    for session in sessions {
        let text = strip_ansi(&read_log(&state.data_dir, &session.id));
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                matches.push(TermMatch {
                    session_id: session.id.clone(),
                    title: session.title.clone(),
                    line: i + 1,
                    text: line.trim().chars().take(240).collect(),
                });
                if matches.len() >= MAX_SEARCH_MATCHES {
                    return Ok(matches);
                }
            }
        }
    }
    Ok(matches)
}

/// The saved pane layout of a project's Terminal screen (JSON written by the UI), if any.
#[tauri::command]
pub fn term_layout_get(state: State<'_, AppState>, project_id: String) -> Result<Option<String>> {
    Ok(state.db.conn().query_row("SELECT layout FROM term_layouts WHERE project_id = ?1", [project_id], |r| r.get(0)).optional()?)
}

#[tauri::command]
pub fn term_layout_set(state: State<'_, AppState>, project_id: String, layout: String) -> Result<()> {
    if layout.len() > MAX_LAYOUT_BYTES {
        return Err("Layout is too large to save".into());
    }
    serde_json::from_str::<serde_json::Value>(&layout)?;
    state.db.conn().execute(
        "INSERT INTO term_layouts (project_id, layout, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET layout = excluded.layout, updated_at = excluded.updated_at",
        params![project_id, layout, db::now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_reads_is_reassembled() {
        let bytes = "héllo ✓".as_bytes();
        let mut pending = bytes[..2].to_vec(); // "h" plus half of "é"
        assert_eq!(take_utf8(&mut pending), "h");
        pending.extend_from_slice(&bytes[2..]);
        assert_eq!(take_utf8(&mut pending), "éllo ✓");
        assert!(pending.is_empty());
        let mut invalid = vec![b'a', 0xff, b'b'];
        assert_eq!(take_utf8(&mut invalid), "a\u{fffd}b");
    }

    #[test]
    fn the_first_cursor_query_is_answered_once_and_removed() {
        let mut h = Handshake::new(true);
        let mut first = b"\x1b[?9001h\x1b[6nMicrosoft Windows".to_vec();
        assert!(h.answer(&mut first));
        assert_eq!(first, b"\x1b[?9001hMicrosoft Windows");
        // A later query comes from a program in the terminal and is left for the screen.
        let mut later = b"\x1b[6n".to_vec();
        assert!(!h.answer(&mut later));
        assert_eq!(later, b"\x1b[6n");
        // Off on Unix, and gives up after the start of the session.
        let mut unix = b"\x1b[6n".to_vec();
        assert!(!Handshake::new(false).answer(&mut unix));
        let mut h = Handshake::new(true);
        assert!(!h.answer(&mut vec![b'x'; 5000]));
        assert!(!h.answer(&mut b"\x1b[6n".to_vec()));
    }

    #[test]
    fn ansi_sequences_are_removed_for_search() {
        let raw = "\x1b]0;title\x07\x1b[1;32mok\x1b[0m done\r\n\x1b[?2004hprompt$ \x1b[K";
        assert_eq!(strip_ansi(raw), "ok done\nprompt$ ");
    }

    #[test]
    fn scrollback_is_cut_back_when_it_passes_the_limit() {
        let path = std::env::temp_dir().join(format!("nexus-log-{}.log", db::new_id()));
        let mut log = Log::create(path.clone()).unwrap();
        let line = format!("{}\n", "x".repeat(1023));
        let mut written = 0u64;
        while written <= SCROLLBACK_LIMIT + 4096 {
            written = log.append(line.as_bytes());
        }
        log.append(b"the newest line\n");
        let on_disk = fs::metadata(&path).unwrap().len();
        assert!(on_disk <= SCROLLBACK_KEEP + 8192, "file was trimmed, is {on_disk}");
        assert_eq!(log.total, written + 16, "stream position keeps counting past the trim");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with('x') && text.ends_with("the newest line\n"), "cut at a line boundary, newest output kept");
        fs::remove_file(path).unwrap();
    }
}

/// Real shells in real pseudo-terminals (openpty on Unix, ConPTY on Windows) on a mock Tauri app.
#[cfg(test)]
mod pty_tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    /// A plain, always-present shell so the tests do not depend on the user's own.
    const TEST_SHELL: &str = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };

    struct Harness {
        app: tauri::App<MockRuntime>,
        project: PathBuf,
    }

    impl Harness {
        fn new() -> Self {
            let project = std::env::temp_dir().join(format!("nexus-term-{}", db::new_id()));
            fs::create_dir_all(&project).unwrap();
            let project = platform::paths::canonical(&project).unwrap();
            fs::create_dir_all(project.join("sub")).unwrap();
            let app = mock_builder().build(mock_context(noop_assets())).unwrap();
            let state = AppState::for_tests();
            state
                .db
                .conn()
                .execute("INSERT INTO projects (id, name, path, added_at) VALUES ('p', 'demo', ?1, '')", [project.to_string_lossy()])
                .unwrap();
            app.manage(state);
            Harness { app, project }
        }

        fn state(&self) -> State<'_, AppState> {
            self.app.state::<AppState>()
        }

        fn open(&self, cwd: Option<&str>) -> Result<Session> {
            open(self.app.handle(), "p", cwd, 80, 24, Some((TEST_SHELL.into(), vec![])))
        }

        /// Type one command line and press Enter the way this system's terminals do.
        fn line(&self, id: &str, text: &str) {
            self.send(id, &format!("{text}{}", if cfg!(windows) { "\r\n" } else { "\n" }));
        }

        fn send(&self, id: &str, text: &str) {
            term_write(self.state(), id.to_string(), text.to_string()).unwrap();
        }

        fn output(&self, id: &str) -> String {
            strip_ansi(&term_scrollback(self.state(), id.to_string()).unwrap().data)
        }

        fn wait<T>(&self, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
            let started = Instant::now();
            loop {
                if let Some(v) = check() {
                    return v;
                }
                assert!(started.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(40));
            }
        }

        fn wait_output(&self, id: &str, needle: &str) -> String {
            self.wait(&format!("output containing {needle:?}"), || Some(self.output(id)).filter(|o| o.contains(needle)))
        }

        fn wait_exit(&self, id: &str) -> Session {
            self.wait("the shell to exit", || Some(load(&self.state().db.conn(), id).unwrap()).filter(|s| s.status != "running"))
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let state = self.state();
            for id in state.terminal.live_ids() {
                let _ = term_kill(state.clone(), id);
            }
            let _ = fs::remove_dir_all(&self.project);
            let _ = fs::remove_dir_all(&state.data_dir);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_starts_in_the_project_runs_input_and_reports_its_exit_code() {
        let h = Harness::new();
        let s = h.open(Some("sub")).unwrap();
        assert_eq!((s.status.as_str(), s.cwd.as_str(), s.shell.as_str()), ("running", "sub", "/bin/sh"));
        assert!(s.pid.is_some());

        // Arithmetic proves the shell evaluated the line rather than the terminal echoing it.
        h.send(&s.id, "echo sum-$((40+2)); pwd\n");
        let out = h.wait_output(&s.id, "sum-42");
        assert!(h.wait_output(&s.id, "/sub").contains(&format!("{}/sub", h.project.display())), "{out}");

        h.send(&s.id, "exit 7\n");
        let ended = h.wait_exit(&s.id);
        assert_eq!((ended.status.as_str(), ended.exit_code, ended.pid), ("exited", Some(7), None));
        assert!(ended.ended_at.is_some());
        assert!(h.state().terminal.live_ids().is_empty());
        // Scrollback outlives the process; writing to it is refused.
        assert!(h.output(&s.id).contains("sum-42"));
        assert!(term_write(h.state(), s.id.clone(), "echo no\n".into()).unwrap_err().to_string().contains("no longer running"));
        assert_eq!(term_list(h.state(), "p".into()).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn resize_reaches_the_shell() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        h.send(&s.id, "stty size\n");
        h.wait_output(&s.id, "24 80");
        term_resize(h.state(), s.id.clone(), 132, 43).unwrap();
        h.send(&s.id, "stty size\n");
        h.wait_output(&s.id, "43 132");
    }

    #[cfg(unix)]
    #[test]
    fn kill_stops_foreground_and_background_jobs() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        // Unique durations so the processes can be found; one job in the background, one in front.
        let (bg, fg) = (format!("31{}", std::process::id()), format!("32{}", std::process::id()));
        h.send(&s.id, &format!("sleep {bg} &\nsleep {fg}\n"));
        let running = |n: &str| {
            std::process::Command::new("pgrep")
                .args(["-f", &format!("sleep {n}")])
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        h.wait("both jobs to start", || (running(&bg) && running(&fg)).then_some(()));

        term_kill(h.state(), s.id.clone()).unwrap();
        let ended = h.wait_exit(&s.id);
        assert_eq!((ended.status.as_str(), ended.exit_code), ("exited", None), "a stopped shell reports no exit code");
        h.wait("both jobs to die", || (!running(&bg) && !running(&fg)).then_some(()));
        assert!(h.output(&s.id).contains("sleep"), "scrollback is kept after a kill");
    }

    #[test]
    fn close_deletes_the_session_and_its_scrollback() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        h.line(&s.id, "echo keepsake");
        h.wait_output(&s.id, "keepsake");
        let log = log_path(&h.state().data_dir, &s.id);
        assert!(log.is_file());
        term_close(h.app.handle().clone(), h.state(), s.id.clone()).unwrap();
        assert!(term_list(h.state(), "p".into()).unwrap().is_empty());
        h.wait("the shell to stop", || h.state().terminal.live_ids().is_empty().then_some(()));
        assert!(!log.exists(), "scrollback file is removed and not recreated by the draining reader");
        assert!(term_scrollback(h.state(), s.id).is_err());
    }

    #[test]
    fn removing_the_project_stops_its_shells() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        h.line(&s.id, "echo alive");
        h.wait_output(&s.id, "alive");
        h.state().db.conn().execute("DELETE FROM projects WHERE id = 'p'", []).unwrap();
        assert!(term_list(h.state(), "p".into()).unwrap().is_empty(), "sessions go with the project");
        h.wait("the orphaned shell to stop", || h.state().terminal.live_ids().is_empty().then_some(()));
        assert!(!log_path(&h.state().data_dir, &s.id).exists());
    }

    #[test]
    fn start_folder_is_confined_to_the_project() {
        let h = Harness::new();
        for bad in ["..", "../..", "sub/../../.."] {
            assert!(h.open(Some(bad)).is_err(), "{bad} must be refused");
        }
        assert!(h.open(Some("missing")).is_err());
        assert!(open(h.app.handle(), "nope", None, 80, 24, None).unwrap_err().to_string().contains("Project not found"));
        assert!(h.state().terminal.live_ids().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn search_finds_output_across_sessions_and_layout_round_trips() {
        let h = Harness::new();
        let (a, b) = (h.open(None).unwrap(), h.open(None).unwrap());
        term_rename(h.state(), b.id.clone(), "  build  ".into()).unwrap();
        h.send(&a.id, "echo Needle-ONE\n");
        h.send(&b.id, "printf '\\033[31mneedle-two\\033[0m\\n'\n");
        h.wait_output(&a.id, "Needle-ONE");
        h.wait_output(&b.id, "needle-two");
        let found = term_search(h.state(), "p".into(), "NEEDLE-".into()).unwrap();
        assert!(found.iter().any(|m| m.session_id == a.id && m.text.contains("Needle-ONE")));
        assert!(
            found.iter().any(|m| m.session_id == b.id && m.title == "build" && m.text.contains("needle-two")),
            "colour codes do not hide matches"
        );
        assert!(term_search(h.state(), "p".into(), "  ".into()).unwrap().is_empty());

        assert_eq!(term_layout_get(h.state(), "p".into()).unwrap(), None);
        term_layout_set(h.state(), "p".into(), r#"{"tabs":[1]}"#.into()).unwrap();
        term_layout_set(h.state(), "p".into(), r#"{"tabs":[2]}"#.into()).unwrap();
        assert_eq!(term_layout_get(h.state(), "p".into()).unwrap().as_deref(), Some(r#"{"tabs":[2]}"#));
        assert!(term_layout_set(h.state(), "p".into(), "not json".into()).is_err());
    }

    #[test]
    fn sessions_left_running_are_marked_ended_and_stray_logs_removed_at_startup() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        h.line(&s.id, "echo before-restart");
        h.wait_output(&s.id, "before-restart");
        let stray = log_path(&h.state().data_dir, "no-such-session");
        fs::write(&stray, "orphan").unwrap();

        // What the next launch does. The shell here is still alive, as it would not be after a real restart.
        recover_interrupted(&h.state().db, &h.state().data_dir).unwrap();
        let after = load(&h.state().db.conn(), &s.id).unwrap();
        assert_eq!((after.status.as_str(), after.pid), ("ended", None));
        assert!(!stray.exists());
        assert!(strip_ansi(&read_log(&h.state().data_dir, &s.id)).contains("before-restart"), "scrollback survives");
    }

    /// Windows: Command Prompt in a ConPTY evaluates input, sees its folder, reports its exit code.
    #[cfg(windows)]
    #[test]
    fn a_windows_shell_runs_input_and_reports_its_exit_code() {
        let h = Harness::new();
        let s = h.open(Some("sub")).unwrap();
        assert_eq!((s.status.as_str(), s.cwd.as_str()), ("running", "sub"));
        // Arithmetic proves cmd evaluated the line rather than the terminal echoing it.
        h.line(&s.id, "set /a 40+2");
        h.wait_output(&s.id, "42");
        h.line(&s.id, "cd");
        h.wait_output(&s.id, "\\sub");
        h.line(&s.id, "exit 7");
        let ended = h.wait_exit(&s.id);
        assert_eq!((ended.status.as_str(), ended.exit_code, ended.pid), ("exited", Some(7), None));
        assert!(h.state().terminal.live_ids().is_empty());
        assert!(term_write(h.state(), s.id.clone(), "echo no\r\n".into()).unwrap_err().to_string().contains("no longer running"));
    }

    /// Windows: the console size follows a resize.
    #[cfg(windows)]
    #[test]
    fn resize_reaches_the_windows_console() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        term_resize(h.state(), s.id.clone(), 132, 43).unwrap();
        h.line(&s.id, "mode con");
        h.wait_output(&s.id, "132");
    }

    /// Windows: stopping a terminal ends the program running in it as well as the shell.
    #[cfg(windows)]
    #[test]
    fn kill_stops_a_running_windows_program() {
        let h = Harness::new();
        let s = h.open(None).unwrap();
        h.line(&s.id, "ping -n 120 127.0.0.1");
        h.wait_output(&s.id, "127.0.0.1");
        term_kill(h.state(), s.id.clone()).unwrap();
        let ended = h.wait_exit(&s.id);
        assert_eq!((ended.status.as_str(), ended.exit_code), ("exited", None), "a stopped shell reports no exit code");
    }

    /// Windows: PowerShell is offered and works as a terminal shell.
    #[cfg(windows)]
    #[test]
    fn powershell_starts_and_evaluates_input() {
        let shells = platform::shell::available();
        let Some(ps) = shells.iter().find(|s| s.to_ascii_lowercase().contains("powershell") || s.to_ascii_lowercase().contains("pwsh"))
        else {
            panic!("no PowerShell found among {shells:?}");
        };
        let h = Harness::new();
        let s = open(h.app.handle(), "p", None, 100, 30, Some((ps.clone(), platform::shell::interactive_args(ps)))).unwrap();
        h.line(&s.id, "Write-Output ('ps-' + (40+2))");
        h.wait_output(&s.id, "ps-42");
        h.line(&s.id, "exit 5");
        assert_eq!(h.wait_exit(&s.id).exit_code, Some(5));
    }

    /// Every system: the shells offered exist, and only an offered shell can be started.
    #[test]
    fn only_offered_shells_can_be_started() {
        let h = Harness::new();
        let offered = term_shells();
        assert!(!offered.is_empty());
        let refused = term_open(h.app.handle().clone(), "p".into(), None, 80, 24, Some("/definitely/not/a/shell".into()));
        assert!(refused.unwrap_err().to_string().contains("not available"));
        let s = term_open(h.app.handle().clone(), "p".into(), None, 80, 24, Some(offered[0].clone())).unwrap();
        assert_eq!(s.status, "running");
    }
}
