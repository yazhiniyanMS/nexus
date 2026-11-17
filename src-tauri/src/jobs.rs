//! Shared process runner. Every command NEXUS executes — task steps and agent test commands —
//! goes through here, so streaming, output limits, cancellation and process-tree kill behave the
//! same everywhere. Callers are responsible for obtaining approval before calling it.

use crate::error::Result;
use crate::platform;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{mpsc, watch};

const MAX_OUTPUT_BYTES: usize = 200_000;

pub struct Exec {
    pub output: String,
    pub code: Option<i64>,
    pub cancelled: bool,
}

/// True for environment variables that look like credentials.
pub fn is_secret_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "AUTH"].iter().any(|k| n.contains(k))
        && !matches!(n.as_str(), "SSH_AUTH_SOCK" | "XAUTHORITY")
}

fn spawn_reader<R: AsyncRead + Unpin + Send + 'static>(stream: R, tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Run a shell command, streaming combined output through `on_chunk`.
/// With `scrub_env`, credential-like environment variables are not passed to the command.
pub async fn run_command(
    command: &str,
    cwd: Option<&Path>,
    scrub_env: bool,
    cancel: &mut watch::Receiver<bool>,
    mut on_chunk: impl FnMut(&str),
) -> Result<Exec> {
    let mut cmd = platform::process::shell_command(command, cwd);
    if scrub_env {
        for (name, _) in std::env::vars_os() {
            if is_secret_env(&name.to_string_lossy()) {
                cmd.env_remove(&name);
            }
        }
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, tx.clone());
    }
    drop(tx);

    let mut output = String::new();
    let mut cancelled = false;
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(line) => {
                    if output.len() < MAX_OUTPUT_BYTES {
                        output.push_str(&line);
                        on_chunk(&line);
                        if output.len() >= MAX_OUTPUT_BYTES {
                            output.push_str("\n… output truncated …\n");
                        }
                    }
                }
                None => break,
            },
            _ = cancel.changed() => {
                cancelled = true;
                platform::process::kill_tree(pid);
                let _ = child.start_kill();
                break;
            }
        }
    }
    let status = child.wait().await?;
    if cancelled {
        output.push_str("\n[cancelled]\n");
    }
    Ok(Exec { output, code: status.code().map(i64::from), cancelled })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_credential_variables() {
        for name in ["ANTHROPIC_API_KEY", "GITHUB_TOKEN", "aws_secret_access_key", "DB_PASSWORD", "NPM_AUTH"] {
            assert!(is_secret_env(name), "{name}");
        }
        for name in ["PATH", "HOME", "SSH_AUTH_SOCK", "LANG", "CARGO_HOME"] {
            assert!(!is_secret_env(name), "{name}");
        }
    }

    #[tokio::test]
    async fn scrubs_credentials_from_the_command_environment() {
        std::env::set_var("NEXUS_TEST_API_KEY", "should-not-leak");
        std::env::set_var("NEXUS_TEST_PLAIN", "visible");
        let (_tx, mut rx) = watch::channel(false);
        let cmd = if cfg!(windows) {
            "echo key=[%NEXUS_TEST_API_KEY%] plain=[%NEXUS_TEST_PLAIN%]"
        } else {
            "echo key=[$NEXUS_TEST_API_KEY] plain=[$NEXUS_TEST_PLAIN]"
        };
        let scrubbed = run_command(cmd, None, true, &mut rx, |_| {}).await.unwrap();
        // cmd.exe prints an unset %VAR% as written; sh prints nothing. Either way the value is gone.
        assert!(!scrubbed.output.contains("should-not-leak"), "{}", scrubbed.output);
        assert!(scrubbed.output.contains("plain=[visible]"), "{}", scrubbed.output);
        let kept = run_command(cmd, None, false, &mut rx, |_| {}).await.unwrap();
        assert!(kept.output.contains("should-not-leak"));
    }
}
