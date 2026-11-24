//! Shared process runner. Every command NEXUS executes for a task step or an agent goes through
//! here, so the security level, environment, time limit, output limit, cancellation and
//! process-tree cleanup behave the same everywhere. Callers obtain approval before calling it.

use crate::error::{AppError, Result};
use crate::exec::{self, sandbox, Level, Policy};
use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{mpsc, watch};

pub struct Exec {
    pub output: String,
    pub code: Option<i64>,
    pub cancelled: bool,
    /// The command ran past its time limit and was stopped.
    pub timed_out: bool,
}

/// What to run, where, and under which policy.
pub struct RunSpec<'a> {
    pub command: &'a str,
    pub cwd: &'a Path,
    pub policy: &'a Policy,
}

/// How long a finished command's leftover processes may keep its output open before they are
/// stopped. Without a limit, `server &` in a command would keep the step running forever.
const LEFTOVER_GRACE: Duration = Duration::from_secs(3);
/// How long a command asked to stop (timeout, cancel, shutdown) gets before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(2);

/// Forward a stream in pieces of at most 8 KB. Reading by line would let one long line (binary
/// output, a progress bar, minified code) grow without bound before anything is cut off.
fn spawn_reader<R: AsyncRead + Unpin + Send + 'static>(stream: R, tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        let mut stream = stream;
        let mut buf = vec![0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    carry.extend_from_slice(&buf[..n]);
                    let text = take_complete_utf8(&mut carry);
                    if !text.is_empty() && tx.send(text).is_err() {
                        return;
                    }
                }
            }
        }
        if !carry.is_empty() {
            let _ = tx.send(String::from_utf8_lossy(&carry).into_owned());
        }
    });
}

/// Decode the longest prefix that does not end inside a multi-byte character, leaving the
/// incomplete tail for the next read. Invalid bytes become U+FFFD as usual.
fn take_complete_utf8(bytes: &mut Vec<u8>) -> String {
    let mut cut = bytes.len();
    // A UTF-8 character is at most 4 bytes, so only the last 3 can belong to an unfinished one.
    for back in 1..=bytes.len().min(3) {
        let b = bytes[bytes.len() - back];
        if b & 0xC0 == 0x80 {
            continue; // continuation byte: keep looking for the lead byte
        }
        let width = if b >= 0xF0 {
            4
        } else if b >= 0xE0 {
            3
        } else if b >= 0xC0 {
            2
        } else {
            1
        };
        if width > back {
            cut = bytes.len() - back;
        }
        break;
    }
    let rest = bytes.split_off(cut);
    let text = String::from_utf8_lossy(bytes).into_owned();
    *bytes = rest;
    text
}

// ------------------------------------------------------------------ running jobs

/// A command that is running, recorded on disk so one left behind by a crash can be stopped at
/// the next start. `pgid` is the process group it leads; `started` is when (Unix seconds).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct JobRecord {
    pub pgid: u32,
    pub started: u64,
}

fn running() -> &'static Mutex<HashMap<u32, JobRecord>> {
    static R: OnceLock<Mutex<HashMap<u32, JobRecord>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

static RECORDS: OnceLock<PathBuf> = OnceLock::new();
const RECORDS_FILE: &str = "running-jobs.json";

fn save_records(jobs: &HashMap<u32, JobRecord>) {
    if let Some(path) = RECORDS.get() {
        let list: Vec<&JobRecord> = jobs.values().collect();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&list).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn register(pid: u32) {
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut jobs = running().lock().unwrap_or_else(|e| e.into_inner());
    jobs.insert(pid, JobRecord { pgid: pid, started });
    save_records(&jobs);
}

fn unregister(pid: u32) {
    let mut jobs = running().lock().unwrap_or_else(|e| e.into_inner());
    jobs.remove(&pid);
    save_records(&jobs);
}

/// Called once at start-up: stop commands a previous run left behind (it crashed or was killed
/// before it could stop them), then record this run's commands in `data_dir`.
pub fn init(data_dir: &Path) {
    let path = data_dir.join(RECORDS_FILE);
    reap_file(&path);
    let _ = RECORDS.set(path);
}

/// Stop what the records in `path` describe, then delete the file.
fn reap_file(path: &Path) {
    if let Ok(text) = std::fs::read_to_string(path) {
        let records: Vec<JobRecord> = serde_json::from_str(&text).unwrap_or_default();
        reap(&records);
    }
    let _ = std::fs::remove_file(path);
}

/// Members of each recorded group that started no earlier than the job did. A process group id
/// cannot be reused while any member of the group is alive, so a member that is old enough
/// belongs to that job; younger groups with a reused id are not touched.
#[cfg(unix)]
pub fn leftovers(records: &[JobRecord], ps_output: &str, now: u64) -> Vec<i32> {
    let mut out = vec![];
    for line in ps_output.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(pgid), Some(etime)) = (it.next(), it.next(), it.next()) else { continue };
        let (Ok(pid), Ok(pgid)) = (pid.parse::<i32>(), pgid.parse::<u32>()) else { continue };
        let Some(age) = parse_etime(etime) else { continue };
        let started = now.saturating_sub(age);
        if records.iter().any(|r| r.pgid == pgid && pgid > 1 && started + 2 >= r.started) {
            out.push(pid);
        }
    }
    out
}

/// `ps` elapsed time, `[[dd-]hh:]mm:ss`, in seconds.
#[cfg(unix)]
fn parse_etime(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(days * 86_400 + secs)
}

fn reap(records: &[JobRecord]) {
    #[cfg(unix)]
    if !records.is_empty() {
        let Ok(out) = std::process::Command::new("ps").args(["-axo", "pid=,pgid=,etime="]).output() else { return };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        for pid in leftovers(records, &String::from_utf8_lossy(&out.stdout), now) {
            // SAFETY: plain signal delivery to a process a previous run of this app started.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
    // Windows has no process groups to identify a job's leftovers by; nothing is guessed.
    #[cfg(not(unix))]
    let _ = records;
}

/// Stop every running command now (the app is quitting). Process trees are killed outright.
pub fn stop_all() {
    stop_where(|_| true);
}

/// Kill the trees of the running commands whose process id matches.
fn stop_where(matches: impl Fn(u32) -> bool) {
    let pids: Vec<u32> = running().lock().unwrap_or_else(|e| e.into_inner()).keys().copied().filter(|p| matches(*p)).collect();
    for pid in pids {
        platform::process::kill_tree(Some(pid));
    }
}

/// Number of commands running now.
#[cfg(all(test, unix))]
pub fn running_count() -> usize {
    running().lock().unwrap_or_else(|e| e.into_inner()).len()
}

// ------------------------------------------------------------------ building the command

/// Limits applied inside the child before it starts the command (Unix).
#[cfg(unix)]
fn apply_limits(cmd: &mut tokio::process::Command, policy: &Policy) {
    let level = policy.level;
    let (file_bytes, memory_bytes) = (policy.max_file_bytes, policy.memory_bytes);
    fn set(resource: libc::c_int, value: u64) {
        let lim = libc::rlimit { rlim_cur: value as libc::rlim_t, rlim_max: value as libc::rlim_t };
        // SAFETY: setrlimit is async-signal-safe and only affects this child.
        unsafe { libc::setrlimit(resource as _, &lim) };
    }
    // SAFETY: the closure runs between fork and exec and calls only setrlimit.
    unsafe {
        cmd.pre_exec(move || {
            set(libc::RLIMIT_CORE as libc::c_int, 0);
            if level != Level::Trusted {
                set(libc::RLIMIT_FSIZE as libc::c_int, file_bytes);
            }
            #[cfg(target_os = "linux")]
            if level == Level::Isolated {
                set(libc::RLIMIT_AS as libc::c_int, memory_bytes);
            }
            let _ = memory_bytes;
            Ok(())
        });
    }
}

/// The process to start for `spec`, plus the line recorded at the top of its output that says
/// exactly how it ran.
fn build(spec: &RunSpec) -> Result<(tokio::process::Command, String)> {
    let policy = spec.policy;
    exec::available(policy.level).map_err(AppError::Msg)?;
    if !spec.cwd.is_dir() {
        return Err(AppError::Msg(format!("The folder to run in does not exist: {}", spec.cwd.display())));
    }
    let mut cmd = match policy.level {
        Level::Trusted => platform::process::shell_command(spec.command, Some(spec.cwd)),
        level => {
            let (shell, fixed) = platform::process::shell_invocation();
            let mut inner: Vec<OsString> = vec![shell.into()];
            inner.extend(fixed.iter().map(OsString::from));
            inner.push(spec.command.into());
            let home = platform::paths::home();
            let confinement = sandbox::confinement(level, spec.cwd, policy.data_dir.as_deref(), home.as_deref());
            let (program, args) = sandbox::wrap(sandbox::backend(), &confinement, spec.cwd, &inner)
                .ok_or_else(|| AppError::Msg(exec::available(level).err().unwrap_or_default()))?;
            let mut cmd = tokio::process::Command::new(program);
            cmd.args(args).current_dir(spec.cwd);
            platform::process::detach_tree(&mut cmd);
            cmd
        }
    };
    let env = exec::env::build(policy.level, &policy.allowed_env);
    let allowed_present =
        env.iter().filter(|(n, _)| policy.allowed_env.iter().any(|a| a.eq_ignore_ascii_case(&n.to_string_lossy()))).count();
    let removed = std::env::vars_os().count().saturating_sub(env.len());
    cmd.env_clear().envs(env).env("PATH", platform::env::search_path());
    #[cfg(unix)]
    apply_limits(&mut cmd, policy);
    let header = format!(
        "[NEXUS] {} · time limit {} · environment: {} variable(s) withheld{}\n",
        exec::describe(policy.level),
        exec::format_duration(policy.timeout),
        removed,
        if allowed_present > 0 { format!(", {allowed_present} passed because you allowed them") } else { String::new() }
    );
    Ok((cmd, header))
}

/// Run a command under its policy, streaming combined output through `on_chunk`. The first chunk
/// is the `[NEXUS]` line saying how it runs. Ends when the command (and anything holding its
/// output) has finished, when it is cancelled, or when its time limit passes; in the last two
/// cases its whole process tree is stopped first.
pub async fn run_command(spec: &RunSpec<'_>, cancel: &mut watch::Receiver<bool>, mut on_chunk: impl FnMut(&str)) -> Result<Exec> {
    let (mut cmd, header) = build(spec)?;
    let limit = spec.policy.output_limit;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AppError::Msg("The shell or sandbox program could not be found".into()),
        std::io::ErrorKind::PermissionDenied => AppError::Msg(format!("Not allowed to start the command: {e}")),
        _ => AppError::Io(e),
    })?;
    let pid = child.id();
    if let Some(pid) = pid {
        register(pid);
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, tx.clone());
    }
    drop(tx);

    let mut output = String::new();
    let mut truncated = false;
    // `notice` text (how the command ran, why it stopped) is always kept, past the limit too.
    let mut keep = |output: &mut String, chunk: &str, notice: bool| {
        if notice {
            output.push_str(chunk);
            on_chunk(chunk);
            return;
        }
        if output.len() < limit {
            let room = limit - output.len();
            let mut end = chunk.len().min(room);
            while !chunk.is_char_boundary(end) {
                end -= 1;
            }
            output.push_str(&chunk[..end]);
            on_chunk(&chunk[..end]);
            if output.len() >= limit && !truncated {
                truncated = true;
                let note = format!("\n… output truncated at {} KB (Settings → Execution) …\n", limit / 1024);
                output.push_str(&note);
                on_chunk(&note);
            }
        }
    };
    keep(&mut output, &header, true);

    let (mut cancelled, mut timed_out) = (false, false);
    let mut status = None;
    let mut leftovers_stopped = false;
    let deadline = tokio::time::sleep(spec.policy.timeout);
    tokio::pin!(deadline);
    loop {
        // After the command itself has exited, wait a little for its output, then stop whatever
        // it left behind that still holds the output open.
        let grace = async {
            match status {
                Some(_) => tokio::time::sleep(LEFTOVER_GRACE).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            chunk = rx.recv() => match chunk {
                Some(chunk) => keep(&mut output, &chunk, false),
                None => break,
            },
            exit = child.wait(), if status.is_none() => status = Some(exit?),
            _ = grace => {
                if leftovers_stopped {
                    // Something outside the process tree still holds the output; stop waiting.
                    break;
                }
                leftovers_stopped = true;
                platform::process::kill_tree(pid);
                keep(&mut output, "\n[NEXUS stopped processes the command left running in the background]\n", true);
            }
            _ = &mut deadline, if status.is_none() => {
                timed_out = true;
                let note = format!(
                    "\n[NEXUS stopped the command: it ran longer than its time limit of {}. Everything it started was stopped too.]\n",
                    exec::format_duration(spec.policy.timeout)
                );
                platform::process::stop_tree(pid, STOP_GRACE).await;
                let _ = child.start_kill();
                keep(&mut output, &note, true);
                break;
            }
            _ = cancel.changed() => {
                cancelled = true;
                platform::process::stop_tree(pid, STOP_GRACE).await;
                let _ = child.start_kill();
                break;
            }
        }
    }
    let status = match status {
        Some(s) => s,
        None => child.wait().await?,
    };
    if let Some(pid) = pid {
        unregister(pid);
    }
    if cancelled {
        keep(&mut output, "\n[cancelled]\n", true);
    }
    let code = if timed_out || cancelled { None } else { status.code().map(i64::from) };
    Ok(Exec { output, code, cancelled, timed_out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn policy(level: Level, timeout: Duration) -> Policy {
        Policy {
            level,
            timeout,
            output_limit: 200 * 1024,
            allowed_env: vec![],
            max_file_bytes: 64 * 1024 * 1024,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            data_dir: None,
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-{label}-{}", crate::db::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    async fn run(command: &str, cwd: &Path, policy: &Policy) -> Result<Exec> {
        let (_tx, mut rx) = watch::channel(false);
        run_command(&RunSpec { command, cwd, policy }, &mut rx, |_| {}).await
    }

    /// Sandbox tests run where a backend exists. CI sets NEXUS_REQUIRE_SANDBOX on runners that
    /// install one, so a missing backend there fails instead of silently skipping.
    fn sandbox_or_skip() -> bool {
        if sandbox::backend() != &sandbox::Backend::None {
            return true;
        }
        // Set and non-empty means required (workflows set it to "" where no backend exists).
        let required = std::env::var("NEXUS_REQUIRE_SANDBOX").is_ok_and(|v| !v.is_empty());
        assert!(!required, "a sandbox backend was required but none was found");
        false
    }

    #[cfg(unix)]
    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    async fn wait_for_pid(file: &Path) -> i32 {
        for _ in 0..100 {
            if let Some(pid) = std::fs::read_to_string(file).ok().and_then(|t| t.trim().parse().ok()) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the test command never wrote its pid");
    }

    #[test]
    fn utf8_is_never_split_across_reads() {
        let text = "aé€😀z";
        let (mut carry, mut out) = (Vec::new(), String::new());
        for b in text.as_bytes() {
            carry.push(*b);
            out.push_str(&take_complete_utf8(&mut carry));
        }
        assert!(carry.is_empty());
        assert_eq!(out, text);
        let mut bad = vec![b'o', b'k', 0xFF, b'!'];
        assert_eq!(take_complete_utf8(&mut bad), "ok\u{FFFD}!");
    }

    /// Every command's record starts with how it ran.
    #[tokio::test]
    async fn the_record_says_how_the_command_ran() {
        let dir = scratch("hdr");
        let e = run("echo hi", &dir, &policy(Level::Trusted, Duration::from_secs(30))).await.unwrap();
        assert!(
            e.output.starts_with("[NEXUS] Trusted:") && e.output.contains("not a sandbox") && e.output.contains("time limit 30 s"),
            "{}",
            e.output
        );
        assert_eq!(e.code, Some(0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn credentials_are_withheld_unless_allowed() {
        std::env::set_var("NEXUS_JOBTEST_API_KEY", "should-not-leak");
        std::env::set_var("NEXUS_JOBTEST_DB", "postgres://u:pw-should-not-leak@h/db");
        std::env::set_var("NEXUS_JOBTEST_PLAIN", "visible");
        let dir = scratch("env");
        let cmd = if cfg!(windows) {
            "echo key=[%NEXUS_JOBTEST_API_KEY%] db=[%NEXUS_JOBTEST_DB%] plain=[%NEXUS_JOBTEST_PLAIN%]"
        } else {
            "echo key=[$NEXUS_JOBTEST_API_KEY] db=[$NEXUS_JOBTEST_DB] plain=[$NEXUS_JOBTEST_PLAIN]"
        };
        let mut p = policy(Level::Trusted, Duration::from_secs(30));
        let e = run(cmd, &dir, &p).await.unwrap();
        assert!(!e.output.contains("should-not-leak"), "{}", e.output);
        assert!(e.output.contains("plain=[visible]"), "{}", e.output);
        p.allowed_env = vec!["NEXUS_JOBTEST_API_KEY".into()];
        let e = run(cmd, &dir, &p).await.unwrap();
        assert!(e.output.contains("key=[should-not-leak]") && e.output.contains("passed because you allowed them"), "{}", e.output);
        assert!(!e.output.contains("pw-should-not-leak"), "only the allowed variable passes");
        if sandbox_or_skip() {
            // Restricted: allow-list only, so even the plain variable is gone.
            let e = run(cmd, &dir, &policy(Level::Restricted, Duration::from_secs(30))).await.unwrap();
            assert!(!e.output.contains("visible") && !e.output.contains("should-not-leak"), "{}", e.output);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// One enormous line, and a never-ending stream, are both cut at the limit.
    #[cfg(unix)]
    #[tokio::test]
    async fn output_is_bounded_however_it_arrives() {
        let dir = scratch("out");
        let mut p = policy(Level::Trusted, Duration::from_secs(30));
        p.output_limit = 64 * 1024;
        let e = run("head -c 20000000 /dev/zero | tr '\\0' x", &dir, &p).await.unwrap();
        assert_eq!(e.code, Some(0));
        assert!(e.output.len() < p.output_limit + 2048, "{}", e.output.len());
        assert!(e.output.contains("output truncated at 64 KB"));
        p.timeout = Duration::from_secs(1);
        let started = Instant::now();
        let e = run("yes nexus-output", &dir, &p).await.unwrap();
        assert!(e.timed_out && started.elapsed() < Duration::from_secs(8));
        assert!(e.output.len() < p.output_limit + 2048, "a continuous stream is bounded too: {}", e.output.len());
        assert!(e.output.contains("time limit of 1 s"), "the timeout is recorded even past the output limit");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A command that leaves a process holding its output must still finish, and the leftover
    /// process must be stopped.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_that_leaves_a_background_process_still_finishes() {
        let dir = scratch("leftover");
        let marker = dir.join("marker");
        let started = Instant::now();
        let cmd = format!("(sleep 30; touch '{}') & echo started", marker.display());
        let e = run(&cmd, &dir, &policy(Level::Trusted, Duration::from_secs(60))).await.unwrap();
        assert_eq!(e.code, Some(0));
        assert!(e.output.contains("started") && e.output.contains("left running in the background"), "{}", e.output);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!marker.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_stops_the_command_and_its_children() {
        let dir = scratch("cancel");
        let marker = dir.join("marker");
        let (tx, mut rx) = watch::channel(false);
        let cmd = format!("sleep 2 && touch '{}' & sleep 30", marker.display());
        let d = dir.clone();
        let run = tokio::spawn(async move {
            let p = policy(Level::Trusted, Duration::from_secs(60));
            run_command(&RunSpec { command: &cmd, cwd: &d, policy: &p }, &mut rx, |_| {}).await.unwrap()
        });
        tokio::time::sleep(Duration::from_millis(400)).await;
        tx.send(true).unwrap();
        let e = tokio::time::timeout(Duration::from_secs(6), run).await.expect("cancel must return promptly").unwrap();
        assert!(e.cancelled && e.code.is_none());
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "a child of the cancelled command kept running");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The time limit stops the command and its children; nothing it started keeps running.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_past_its_time_limit_is_stopped_with_everything_it_started() {
        let dir = scratch("timeout");
        let marker = dir.join("marker");
        let started = Instant::now();
        let cmd = format!("sleep 3 && touch '{}' & sleep 60", marker.display());
        let e = run(&cmd, &dir, &policy(Level::Trusted, Duration::from_secs(1))).await.unwrap();
        assert!(e.timed_out && !e.cancelled && e.code.is_none());
        assert!(started.elapsed() < Duration::from_secs(6), "{:?}", started.elapsed());
        assert!(e.output.contains("ran longer than its time limit of 1 s"), "{}", e.output);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "a child outlived the timeout");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs on every system, Windows included: the time limit ends a command that would run for
    /// a minute, and the run reports it.
    #[tokio::test]
    async fn the_time_limit_applies_on_every_system() {
        let dir = scratch("timeout-any");
        let cmd = if cfg!(windows) { "ping -n 60 127.0.0.1 >nul" } else { "sleep 60" };
        let started = Instant::now();
        let e = run(cmd, &dir, &policy(Level::Trusted, Duration::from_secs(1))).await.unwrap();
        assert!(e.timed_out && e.code.is_none(), "{}", e.output);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A command that ignores the polite request is killed after the grace period.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_that_ignores_termination_is_killed() {
        let dir = scratch("stubborn");
        let pidfile = dir.join("pid");
        let cmd = format!("trap '' TERM; echo $$ > '{}'; while true; do sleep 0.2; done", pidfile.display());
        let started = Instant::now();
        let e = run(&cmd, &dir, &policy(Level::Trusted, Duration::from_secs(1))).await.unwrap();
        let pid = wait_for_pid(&pidfile).await;
        assert!(e.timed_out);
        assert!(started.elapsed() < Duration::from_secs(1) + STOP_GRACE + Duration::from_secs(3), "{:?}", started.elapsed());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!alive(pid), "the shell that ignored SIGTERM is still running");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A child that moved into its own session (so it is not in the command's process group) is
    /// still found through its parent and stopped.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_left_the_process_group_is_stopped_too() {
        let dir = scratch("setsid");
        let pidfile = dir.join("pid");
        let script = format!(
            "perl -MPOSIX -e 'if (fork() == 0) {{ POSIX::setsid(); open(my $f, \">\", $ARGV[0]); print $f $$; close $f; sleep 300; exit 0 }} sleep 300' '{}'",
            pidfile.display()
        );
        let e = run(&script, &dir, &policy(Level::Trusted, Duration::from_secs(1))).await.unwrap();
        assert!(e.timed_out);
        let pid = wait_for_pid(&pidfile).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!alive(pid), "the escaped child {pid} is still running");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn missing_folders_programs_and_permissions_fail_cleanly() {
        let dir = scratch("errors");
        let p = policy(Level::Trusted, Duration::from_secs(30));
        let err = run("echo hi", &dir.join("gone"), &p).await.err().expect("must refuse").to_string();
        assert!(err.contains("does not exist"), "{err}");
        let e = run("nexus-no-such-program-7f3a", &dir, &p).await.unwrap();
        assert!(matches!(e.code, Some(c) if c != 0), "{:?}", e.code);
        #[cfg(unix)]
        {
            std::fs::write(dir.join("script.sh"), "#!/bin/sh\necho ran\n").unwrap();
            let e = run("./script.sh", &dir, &p).await.unwrap();
            assert_eq!(e.code, Some(126), "not executable: {}", e.output);
            assert!(!e.output.contains("\nran"));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Restricted: the working folder is writable, NEXUS's data folder can be neither read nor
    /// written, and (on macOS) the keychain cannot be reached.
    #[cfg(unix)]
    #[tokio::test]
    async fn restricted_commands_cannot_reach_hidden_data() {
        if !sandbox_or_skip() {
            return;
        }
        let dir = scratch("restricted");
        let work = dir.join("work");
        let data = dir.join("nexus-data");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("nexus.db"), "private transcripts").unwrap();
        let mut p = policy(Level::Restricted, Duration::from_secs(30));
        p.data_dir = Some(data.clone());
        let cmd = format!(
            "echo ok > inside.txt; echo w-in=$?; cat '{d}/nexus.db'; echo r-data=$?; echo x > '{d}/planted'; echo w-data=$?",
            d = data.display()
        );
        let e = run(&cmd, &work, &p).await.unwrap();
        assert!(e.output.contains("w-in=0") && work.join("inside.txt").exists(), "{}", e.output);
        assert!(!e.output.contains("private transcripts") && !e.output.contains("r-data=0"), "{}", e.output);
        assert!(!e.output.contains("w-data=0") && !data.join("planted").exists(), "{}", e.output);
        assert!(e.output.starts_with("[NEXUS] Restricted ("), "{}", e.output);
        #[cfg(target_os = "macos")]
        {
            let e = run("security list-keychains >/dev/null 2>&1; echo keychain=$?", &work, &p).await.unwrap();
            assert!(e.output.contains("keychain=") && !e.output.contains("keychain=0"), "{}", e.output);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The enforcement mechanism itself: with only the working folder writable, a write anywhere
    /// else fails. (Policies also allow temp and caches; this checks the rule underneath.)
    #[cfg(unix)]
    #[tokio::test]
    async fn writes_outside_the_writable_folders_are_refused() {
        if !sandbox_or_skip() {
            return;
        }
        let dir = scratch("confine");
        let (work, outside) = (dir.join("work"), dir.join("outside"));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let c = sandbox::Confinement { writable: vec![work.clone()], hidden: vec![], network: true };
        let inner: Vec<OsString> =
            vec!["sh".into(), "-c".into(), format!("echo a > in.txt; echo b > '{}/out.txt'; echo done", outside.display()).into()];
        let (program, args) = sandbox::wrap(sandbox::backend(), &c, &work, &inner).unwrap();
        let out = tokio::process::Command::new(program).args(args).current_dir(&work).output().await.unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("done"));
        assert!(work.join("in.txt").exists());
        assert!(!outside.join("out.txt").exists(), "a write outside the writable folder succeeded");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Isolated: no network except this computer.
    #[cfg(unix)]
    #[tokio::test]
    async fn isolated_commands_reach_only_this_computer() {
        if !sandbox_or_skip() {
            return;
        }
        let server = crate::lab::testserver::serve(|_| crate::lab::testserver::Resp::json(serde_json::json!({"ok": true}))).await;
        let dir = scratch("isolated");
        // 192.0.2.1 is reserved for documentation and never answers: without isolation curl
        // waits for its time limit (exit 28); with it, the connection is refused at once (exit 7).
        let cmd =
            format!("curl -s -m 4 {}/ >/dev/null; echo local=$?; curl -s -m 4 http://192.0.2.1/ >/dev/null; echo remote=$?", server.url);
        let started = Instant::now();
        let e = run(&cmd, &dir, &policy(Level::Isolated, Duration::from_secs(30))).await.unwrap();
        if cfg!(target_os = "macos") {
            assert!(e.output.contains("local=0"), "macOS: other programs on this computer stay reachable: {}", e.output);
        } else {
            // bubblewrap gives the command its own network namespace: not even this computer.
            assert!(e.output.contains("local=") && !e.output.contains("local=0"), "{}", e.output);
        }
        assert!(e.output.contains("remote=") && !e.output.contains("remote=0") && !e.output.contains("remote=28"), "{}", e.output);
        assert!(started.elapsed() < Duration::from_secs(4), "refused immediately, not timed out: {:?}", started.elapsed());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn leftovers_from_a_crash_are_identified_by_group_and_age() {
        let records = [JobRecord { pgid: 500, started: 1_000 }];
        let ps = "  500   500    00:30\n  501   500    00:20\n  502   500    01:00:00\n  600   600    00:10\n  700   500  1-00:00:00\n";
        // At now=1_040: 500 started 1010, 501 at 1020 (both after the job); 502 started an hour
        // before the job and 700 a day before, so they cannot be its processes; 600 is another group.
        assert_eq!(leftovers(&records, ps, 1_040), [500, 501]);
        assert!(leftovers(&[], ps, 1_040).is_empty());
        assert_eq!(parse_etime("2-03:04:05"), Some(2 * 86_400 + 3 * 3600 + 4 * 60 + 5));
        assert_eq!(parse_etime("bad"), None);
    }

    /// A command left running by a crash (its record still on disk) is stopped at the next start,
    /// children included; an unrelated process is not touched.
    #[cfg(unix)]
    #[tokio::test]
    async fn commands_left_by_a_crash_are_stopped_at_the_next_start() {
        use std::os::unix::process::CommandExt;
        let dir = scratch("crash");
        let mut leader = std::process::Command::new("sh").args(["-c", "sleep 300 & sleep 300"]).process_group(0).spawn().unwrap();
        let mut bystander = std::process::Command::new("sleep").arg("300").spawn().unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let pgid = leader.id();
        let children = platform::process::descendants(pgid);
        assert!(!children.is_empty());
        let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 1;
        let file = dir.join(RECORDS_FILE);
        std::fs::write(&file, serde_json::to_vec(&[JobRecord { pgid, started }]).unwrap()).unwrap();
        reap_file(&file);
        let status = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(Some(s)) = leader.try_wait() {
                    return s;
                }
                tokio::time::sleep(Duration::from_millis(50)).await
            }
        })
        .await
        .expect("the leader was stopped");
        assert!(!status.success());
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(children.iter().all(|c| !alive(*c)), "a child of the crashed job survived");
        assert!(bystander.try_wait().unwrap().is_none(), "an unrelated process was touched");
        assert!(!file.exists(), "the records are cleared");
        let _ = bystander.kill();
        let _ = bystander.wait();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Quitting the app stops every running command: each one is in the registry `stop_all`
    /// walks. (Only this test's own command is stopped here, so tests running in parallel are not.)
    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_everything_ends_running_commands() {
        let dir = scratch("stopall");
        let pidfile = dir.join("pid");
        let (d, cmd) = (dir.clone(), format!("echo $$ > '{}'; sleep 60", pidfile.display()));
        let handle = tokio::spawn(async move { run(&cmd, &d, &policy(Level::Trusted, Duration::from_secs(120))).await.unwrap() });
        let pid = wait_for_pid(&pidfile).await as u32;
        assert!(running_count() >= 1);
        assert!(running().lock().unwrap().contains_key(&pid), "the command is registered");
        stop_where(|p| p == pid);
        let e = tokio::time::timeout(Duration::from_secs(5), handle).await.expect("stopped").unwrap();
        assert_ne!(e.code, Some(0));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
