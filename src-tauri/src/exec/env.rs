//! Environment policy: which variables a command NEXUS starts may see.
//!
//! - **Trusted** keeps the user's environment minus anything that looks like a credential.
//! - **Restricted** and **Isolated** start from an allow-list of variables tools need to find
//!   themselves (PATH, HOME, locale, toolchain homes) and nothing else.
//!
//! At every level the user can name variables that must pass anyway (`allowed`), for example a
//! `GITHUB_TOKEN` a task really needs. A name the user allowed is passed even if it looks secret.

use super::Level;
use std::ffi::OsString;

/// Variables commands may need at Restricted and Isolated. Compared without case on Windows.
const ALLOW_LIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TERM",
    "TZ",
    "TMPDIR",
    "TMP",
    "TEMP",
    "COLORTERM",
    "NO_COLOR",
    "FORCE_COLOR",
    "CI",
    "EDITOR",
    "PAGER",
    // Toolchains locate themselves through these.
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "GOPATH",
    "GOROOT",
    "GOCACHE",
    "GOMODCACHE",
    "GOFLAGS",
    "JAVA_HOME",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "NVM_DIR",
    "NVM_BIN",
    "PNPM_HOME",
    "BUN_INSTALL",
    "DENO_DIR",
    "PYENV_ROOT",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    "PIPX_HOME",
    "POETRY_HOME",
    "GEM_HOME",
    "GEM_PATH",
    "RBENV_ROOT",
    "DEVELOPER_DIR",
    "SDKROOT",
    "MACOSX_DEPLOYMENT_TARGET",
    "DOTNET_ROOT",
    // Windows needs these to start almost anything.
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

/// Prefixes of allow-listed families (`LC_ALL`, `XDG_CONFIG_HOME`, …).
const ALLOW_PREFIXES: &[&str] = &["LC_", "XDG_"];

/// Name segments that mark a credential: `GITHUB_TOKEN`, `AWS_SECRET_ACCESS_KEY`, `NPM_AUTH`, …
/// Matched on whole `_`-separated segments so `GIT_AUTHOR_NAME` or `KEYBOARD_LAYOUT` are not.
const SECRET_SEGMENTS: &[&str] = &[
    "KEY",
    "KEYS",
    "APIKEY",
    "TOKEN",
    "TOKENS",
    "SECRET",
    "SECRETS",
    "PASSWORD",
    "PASSWD",
    "PASS",
    "PASSPHRASE",
    "CREDENTIAL",
    "CREDENTIALS",
    "AUTH",
    "OAUTH",
    "COOKIE",
    "PRIVATE",
    "DSN",
    "SAS",
    "PAT",
];

/// Whole names that carry credentials without a telling segment.
const SECRET_NAMES: &[&str] = &[
    "DATABASE_URL",
    "REDIS_URL",
    "MONGODB_URI",
    "MONGO_URL",
    "MONGO_URI",
    "AMQP_URL",
    "RABBITMQ_URL",
    "POSTGRES_URL",
    "MYSQL_URL",
    "SENTRY_DSN",
    "KUBECONFIG",
    "NETRC",
    "PGPASSFILE",
    "HTPASSWD",
];

/// Variables that look like credentials by name but that the user's own (Trusted) commands rely
/// on: the SSH agent socket for `git push`, the X authority file for GUI tools.
const TRUSTED_EXCEPTIONS: &[&str] = &["SSH_AUTH_SOCK", "XAUTHORITY", "GPG_AGENT_INFO", "GPG_TTY"];

fn upper(name: &str) -> String {
    name.to_ascii_uppercase()
}

/// True for a variable whose name marks it as a credential.
pub fn is_secret_name(name: &str) -> bool {
    let n = upper(name);
    SECRET_NAMES.contains(&n.as_str()) || n.split(['_', '-', '.']).any(|seg| SECRET_SEGMENTS.contains(&seg))
}

/// True for a value that is itself a credential whatever the variable is called: a URL with a
/// password in it, or a well-known token format.
pub fn is_secret_value(value: &str) -> bool {
    let v = value.trim();
    if let Some((_, rest)) = v.split_once("://") {
        if let Some((userinfo, _)) = rest.split_once('@') {
            if userinfo.contains(':') && !userinfo.ends_with(':') {
                return true;
            }
        }
    }
    crate::secrets::detect(v).iter().any(|s| s.kind == crate::secrets::Kind::Token)
}

/// Whether a variable is passed to a command started at `level`.
pub fn passes(level: Level, name: &str, value: &str, allowed: &[String]) -> bool {
    let n = upper(name);
    if n == "NEXUS_E2E_PORT" {
        return false; // the debug test bridge is never handed on
    }
    if allowed.iter().any(|a| upper(a) == n) {
        return true;
    }
    match level {
        Level::Trusted => TRUSTED_EXCEPTIONS.contains(&n.as_str()) || !(is_secret_name(&n) || is_secret_value(value)),
        Level::Restricted | Level::Isolated => {
            (ALLOW_LIST.contains(&n.as_str()) || ALLOW_PREFIXES.iter().any(|p| n.starts_with(p))) && !is_secret_value(value)
        }
    }
}

/// The environment for a command at `level`: built from scratch, never inherited implicitly.
pub fn build(level: Level, allowed: &[String]) -> Vec<(OsString, OsString)> {
    std::env::vars_os().filter(|(name, value)| passes(level, &name.to_string_lossy(), &value.to_string_lossy(), allowed)).collect()
}

/// A user-supplied variable name is valid when it could be a real variable.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_classified_by_segment() {
        for secret in [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GITHUB_TOKEN",
            "aws_secret_access_key",
            "AWS_SESSION_TOKEN",
            "DB_PASSWORD",
            "NPM_AUTH",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "DATABASE_URL",
            "SENTRY_DSN",
            "SSH_AUTH_SOCK",
            "MY_PRIVATE_KEY",
            "HF_TOKEN",
        ] {
            assert!(is_secret_name(secret), "{secret}");
        }
        for plain in [
            "PATH",
            "HOME",
            "GIT_AUTHOR_NAME",
            "KEYBOARD_LAYOUT",
            "PWD",
            "LANG",
            "CARGO_HOME",
            "MONKEY_COUNT",
            "AUTHOR",
            "SSL_CERT_FILE",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            assert!(!is_secret_name(plain), "{plain}");
        }
    }

    #[test]
    fn values_are_recognised_whatever_the_name() {
        assert!(is_secret_value("postgres://app:s3cr3t-pw@db.internal:5432/app"));
        assert!(is_secret_value(&format!("ghp_{}", "x1".repeat(18))));
        assert!(!is_secret_value("postgres://db.internal:5432/app"), "no password, nothing secret");
        assert!(!is_secret_value("https://example.com/path"));
        assert!(!is_secret_value("/usr/local/bin:/usr/bin"));
    }

    #[test]
    fn levels_pass_what_they_promise() {
        let none: Vec<String> = vec![];
        // Trusted: everything except credentials, keeping the SSH agent for the user's own git.
        assert!(passes(Level::Trusted, "MY_APP_MODE", "dev", &none));
        assert!(!passes(Level::Trusted, "OPENAI_API_KEY", "x", &none));
        assert!(passes(Level::Trusted, "SSH_AUTH_SOCK", "/tmp/agent.sock", &none));
        assert!(!passes(Level::Trusted, "APP_DB", "mysql://root:hunter22@localhost/app", &none), "credential URL under a plain name");
        // Restricted: allow-list only; the SSH agent is not handed on.
        assert!(passes(Level::Restricted, "PATH", "/usr/bin", &none));
        assert!(passes(Level::Restricted, "LC_ALL", "en_US.UTF-8", &none));
        assert!(!passes(Level::Restricted, "MY_APP_MODE", "dev", &none));
        assert!(!passes(Level::Restricted, "SSH_AUTH_SOCK", "/tmp/agent.sock", &none));
        // The user's explicit choice wins at every level.
        let allowed = vec!["GITHUB_TOKEN".to_string(), "MY_APP_MODE".to_string()];
        for level in [Level::Trusted, Level::Restricted, Level::Isolated] {
            assert!(passes(level, "GITHUB_TOKEN", "t", &allowed), "{level:?}");
            assert!(passes(level, "my_app_mode", "dev", &allowed), "{level:?} case-insensitive");
            assert!(!passes(level, "NEXUS_E2E_PORT", "1", &["NEXUS_E2E_PORT".into()]), "{level:?} test bridge never passes");
        }
    }

    #[test]
    fn variable_names_are_validated() {
        assert!(valid_name("GITHUB_TOKEN") && valid_name("_X1"));
        assert!(!valid_name("") && !valid_name("1ABC") && !valid_name("A-B") && !valid_name("A B") && !valid_name("A=B"));
    }
}
