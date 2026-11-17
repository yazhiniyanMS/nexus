//! Shell discovery: which shell an interactive terminal starts, per system.

use std::path::Path;

/// Shell names that understand `-l` (login shell), so PATH matches the user's own terminal.
const LOGIN_FLAG_SHELLS: [&str; 4] = ["zsh", "bash", "fish", "ksh"];

fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Arguments for an interactive session of `shell`.
pub fn interactive_args(shell: &str) -> Vec<String> {
    let name = file_stem(shell);
    if cfg!(windows) {
        // PowerShell prints a banner on every start unless told not to.
        if name == "pwsh" || name == "powershell" {
            vec!["-NoLogo".into()]
        } else {
            vec![]
        }
    } else if LOGIN_FLAG_SHELLS.contains(&name.as_str()) {
        vec!["-l".into()]
    } else {
        vec![]
    }
}

/// The user's login shell on Unix: `$SHELL`, then the account database, then `/bin/sh`.
#[cfg(unix)]
pub fn login_shell() -> String {
    let from_env = std::env::var("SHELL").ok().filter(|s| Path::new(s).is_file());
    from_env.or_else(passwd_shell).unwrap_or_else(|| "/bin/sh".into())
}

#[cfg(unix)]
fn passwd_shell() -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage or null; it is read immediately.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_shell.is_null() {
            return None;
        }
        let shell = std::ffi::CStr::from_ptr((*entry).pw_shell).to_string_lossy().into_owned();
        Path::new(&shell).is_file().then_some(shell)
    }
}

/// Windows: PowerShell 7 if installed, then Windows PowerShell, then Command Prompt.
#[cfg(windows)]
fn windows_shell() -> String {
    for name in ["pwsh", "powershell"] {
        if let Some(path) = super::programs::find(name) {
            return path.to_string_lossy().into_owned();
        }
    }
    std::env::var("ComSpec").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".into())
}

/// The shell a new terminal starts, with its arguments.
pub fn interactive() -> (String, Vec<String>) {
    #[cfg(windows)]
    let shell = windows_shell();
    #[cfg(unix)]
    let shell = login_shell();
    let args = interactive_args(&shell);
    (shell, args)
}

/// Shells the user can pick for a terminal on this system, the default first.
pub fn available() -> Vec<String> {
    let mut found = vec![interactive().0];
    #[cfg(windows)]
    {
        for name in ["pwsh", "powershell", "cmd"] {
            if let Some(path) = super::programs::find(name) {
                found.push(path.to_string_lossy().into_owned());
            }
        }
    }
    #[cfg(unix)]
    {
        for path in ["/bin/zsh", "/bin/bash", "/usr/bin/fish", "/bin/sh"] {
            if Path::new(path).is_file() {
                found.push(path.to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    found.retain(|s| seen.insert(s.to_ascii_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_shell_exists_and_is_listed_first() {
        let (shell, _) = interactive();
        assert!(!shell.is_empty());
        assert_eq!(available()[0], shell);
        #[cfg(unix)]
        assert!(Path::new(&shell).is_file(), "{shell}");
    }

    #[test]
    fn arguments_match_the_shell_family() {
        if cfg!(windows) {
            assert_eq!(interactive_args(r"C:\Program Files\PowerShell\7\pwsh.exe"), ["-NoLogo"]);
            assert_eq!(interactive_args(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"), ["-NoLogo"]);
            assert!(interactive_args(r"C:\Windows\System32\cmd.exe").is_empty());
        } else {
            assert_eq!(interactive_args("/bin/zsh"), ["-l"]);
            assert_eq!(interactive_args("/usr/bin/fish"), ["-l"]);
            assert!(interactive_args("/bin/sh").is_empty(), "plain sh is not given a login flag");
        }
    }
}
