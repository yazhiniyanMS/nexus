//! Starting and stopping processes. Unix has process groups, sessions and signals; Windows has
//! none of them, so each operation has its own implementation here and callers use one API.

use std::path::Path;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Keep a console window from flashing up when a GUI app starts a console program (Windows).
pub fn hide_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Make an async child the root of its own tree so [`kill_tree`] reaches everything it starts,
/// without a console window on Windows.
pub fn detach_tree(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}

/// How a command line is handed to the system shell, as (program, fixed arguments).
/// The command itself is appended by [`shell_command`].
pub fn shell_invocation() -> (&'static str, &'static [&'static str]) {
    if cfg!(windows) {
        ("cmd.exe", &["/D", "/S", "/C"])
    } else {
        ("sh", &["-c"])
    }
}

/// What Windows `cmd /S /C` receives: UTF-8 output, then the command, wrapped in one pair of
/// quotes that `/S` strips again so quotes inside the command survive untouched.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn windows_command_line(command: &str) -> String {
    format!("\"chcp 65001>nul & {command}\"")
}

/// A command line run by the system shell (`sh -c` or `cmd /C`), in its own process tree, with
/// the user's search path so tools installed for their terminal are found.
pub fn shell_command(command: &str, cwd: Option<&Path>) -> tokio::process::Command {
    let (program, fixed) = shell_invocation();
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(fixed);
    #[cfg(windows)]
    cmd.raw_arg(windows_command_line(command));
    #[cfg(not(windows))]
    cmd.arg(command);
    detach_tree(&mut cmd);
    cmd.env("PATH", super::env::search_path());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd
}

/// Kill a process and everything it started.
pub fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    // SAFETY: the child was started as its own process-group leader, so this reaches its descendants.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    taskkill(pid, true);
}

/// Ask a process tree to stop, leaving it time to clean up. Follow with [`kill_tree`] if it stays.
pub fn terminate_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    // SAFETY: plain signal delivery to the process group this app created.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    taskkill(pid, false);
}

#[cfg(windows)]
fn taskkill(pid: u32, force: bool) {
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        cmd.arg("/F");
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    hide_window(&mut cmd);
    let _ = cmd.status();
}

/// Every process in a terminal's session, so jobs the shell put in their own process groups are
/// reached too.
#[cfg(unix)]
fn session_members(leader: u32) -> Vec<libc::pid_t> {
    let Ok(out) = std::process::Command::new("ps").args(["-axo", "pid="]).output() else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<libc::pid_t>().ok())
        // SAFETY: getsid only reads process metadata.
        .filter(|pid| unsafe { libc::getsid(*pid) } == leader as libc::pid_t)
        .collect()
}

/// Stop an interactive shell and everything started from it. On Unix: hang up first so programs
/// can clean up, then kill whatever is left in the session. On Windows: end the process tree.
pub fn kill_session(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    {
        let members = session_members(pid);
        // SAFETY: plain signal delivery to processes of this user.
        unsafe {
            for member in &members {
                libc::kill(*member, libc::SIGHUP);
            }
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            let mut all = session_members(pid);
            all.extend(members);
            // SAFETY: as above. A pid that has exited is only signalled if it is still in this session.
            unsafe {
                for member in all {
                    if libc::getsid(member) == pid as libc::pid_t {
                        libc::kill(member, libc::SIGKILL);
                    }
                }
            }
        });
    }
    #[cfg(windows)]
    taskkill(pid, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_command_lines_keep_inner_quotes_and_switch_to_utf8() {
        assert_eq!(windows_command_line(r#"npm run "my test""#), r#""chcp 65001>nul & npm run "my test"""#);
    }

    #[test]
    fn the_shell_matches_the_system() {
        let (program, fixed) = shell_invocation();
        if cfg!(windows) {
            assert_eq!((program, fixed.last().copied()), ("cmd.exe", Some("/C")));
        } else {
            assert_eq!((program, fixed), ("sh", &["-c"][..]));
        }
    }

    /// Runs on every system: the shell evaluates the line, sees the working folder, and reports
    /// its exit code.
    #[tokio::test]
    async fn a_shell_command_runs_in_the_given_folder_and_returns_its_exit_code() {
        let dir = std::env::temp_dir().join(format!("nexus-proc-{}", crate::db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker-file.txt"), "x").unwrap();
        let list = if cfg!(windows) { "dir /b" } else { "ls" };
        let out = shell_command(&format!("{list} && exit 3"), Some(&dir)).output().await.unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("marker-file.txt"));
        assert_eq!(out.status.code(), Some(3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Runs on every system: killing the tree ends a child the shell started, not just the shell.
    #[tokio::test]
    async fn killing_the_tree_ends_grandchildren() {
        let sleeper = if cfg!(windows) { "ping -n 60 127.0.0.1 >nul" } else { "sleep 60" };
        let mut child = shell_command(sleeper, None).stdout(std::process::Stdio::null()).spawn().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let started = std::time::Instant::now();
        kill_tree(child.id());
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await.expect("the shell ended").unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
