//! Isolation backends. Each turns a [`Confinement`] (what a command may write, what it may not
//! see, whether it may use the network) into the program line that enforces it.
//!
//! - macOS: `sandbox-exec` with a generated profile (the Seatbelt sandbox the OS uses for its own
//!   services). Apple marks the command-line tool deprecated; it is present and enforced on
//!   current macOS, and NEXUS checks that it works before offering it.
//! - Linux: `bubblewrap` (`bwrap`) when installed: a read-only view of the system, writable bind
//!   mounts, credential folders hidden behind empty ones, optional network namespace, its own
//!   process namespace, and death with NEXUS.
//! - Windows (and Linux without bubblewrap): no backend. Restricted and Isolated are reported as
//!   unavailable and refused there; NEXUS never falls back to running without isolation.

use super::Level;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    None,
    MacSandbox,
    Bubblewrap(PathBuf),
}

impl Backend {
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Backend::None => None,
            Backend::MacSandbox => Some("macOS sandbox"),
            Backend::Bubblewrap(_) => Some("bubblewrap"),
        }
    }
}

/// The backend this machine offers, found once by actually running it.
pub fn backend() -> &'static Backend {
    static B: OnceLock<Backend> = OnceLock::new();
    B.get_or_init(probe)
}

fn quiet(cmd: &mut std::process::Command) -> bool {
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    crate::platform::process::hide_window(cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

fn probe() -> Backend {
    if cfg!(target_os = "macos") {
        let tool = Path::new("/usr/bin/sandbox-exec");
        if tool.exists() && quiet(std::process::Command::new(tool).args(["-p", "(version 1)(allow default)", "/usr/bin/true"])) {
            return Backend::MacSandbox;
        }
    }
    if cfg!(target_os = "linux") {
        let found = std::env::split_paths(crate::platform::env::search_path()).map(|d| d.join("bwrap")).find(|p| p.is_file());
        if let Some(bwrap) = found {
            // User namespaces can be disabled by the distribution; only offer what works.
            if quiet(std::process::Command::new(&bwrap).args(["--ro-bind", "/", "/", "--dev", "/dev", "--unshare-pid", "--", "true"])) {
                return Backend::Bubblewrap(bwrap);
            }
        }
    }
    Backend::None
}

/// What a confined command may do.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Confinement {
    /// Folders (canonical paths) it may write in. Everything else is read-only.
    pub writable: Vec<PathBuf>,
    /// Folders and files (canonical paths) it may not read at all.
    pub hidden: Vec<PathBuf>,
    /// Whether it may reach other machines. Without it, only this machine (loopback) works.
    pub network: bool,
}

fn canonical_existing(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for p in paths {
        if let Ok(c) = std::fs::canonicalize(&p) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// Tool caches a build may legitimately write to (dependency downloads, compiled artefacts).
const CACHE_DIRS: &[&str] = &[
    ".cargo/registry",
    ".cargo/git",
    ".rustup",
    ".npm",
    ".cache",
    "Library/Caches",
    ".gradle",
    ".m2",
    "go/pkg",
    ".pnpm-store",
    "Library/pnpm",
    ".yarn",
    ".bun",
    ".deno",
    ".nuget/packages",
    ".pub-cache",
    ".local/share/pnpm",
    ".cocoapods",
    ".expo",
];

/// Credentials and secrets that commands at Restricted and Isolated may not read.
const CREDENTIAL_PATHS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".config/gcloud",
    ".azure",
    ".kube",
    ".docker",
    ".netrc",
    ".git-credentials",
    ".config/gh",
    ".config/hub",
    ".password-store",
    "Library/Keychains",
    ".terraform.d",
    ".pypirc",
    ".gem/credentials",
    ".config/op",
    ".vault-token",
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".config/git/credentials",
    ".oci",
    ".config/doctl",
    ".local/share/keyrings",
];

/// Readable at Restricted (package managers need their registry settings) but hidden at Isolated.
const ISOLATED_EXTRA_HIDDEN: &[&str] = &[".npmrc", ".yarnrc", ".yarnrc.yml", ".config/pip", ".pip", ".bunfig.toml"];

/// The confinement for `level` when running in `cwd`. `data_dir` (NEXUS's own database and
/// workspaces) is hidden unless the command runs inside it, which agent workspaces do.
pub fn confinement(level: Level, cwd: &Path, data_dir: Option<&Path>, home: Option<&Path>) -> Confinement {
    if level == Level::Trusted {
        return Confinement { writable: vec![], hidden: vec![], network: true };
    }
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut writable = vec![std::env::temp_dir(), PathBuf::from("/tmp"), PathBuf::from("/var/tmp")];
    // The working folder is writable, unless it is the home folder itself (a task without a
    // project): making all of home writable would undo the point of the level.
    let home_canonical = home.and_then(|h| std::fs::canonicalize(h).ok());
    if home_canonical.as_deref() != Some(cwd.as_path()) {
        writable.push(cwd.clone());
    }
    let mut hidden: Vec<PathBuf> = vec![];
    if let Some(home) = home {
        if level == Level::Restricted {
            writable.extend(CACHE_DIRS.iter().map(|d| home.join(d)));
        }
        hidden.extend(CREDENTIAL_PATHS.iter().map(|d| home.join(d)));
        if level == Level::Isolated {
            hidden.extend(ISOLATED_EXTRA_HIDDEN.iter().map(|d| home.join(d)));
        }
    }
    if let Some(data) = data_dir {
        hidden.push(data.to_path_buf());
    }
    // Linux: the session bus socket lives here, and through it the Secret Service that holds
    // NEXUS's API keys. (macOS: the keychain is blocked in the profile instead.)
    if cfg!(target_os = "linux") {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            hidden.push(PathBuf::from(runtime));
        }
    }
    let writable = canonical_existing(writable);
    // Never hide the folder the command works in (an agent workspace lives in NEXUS's data
    // folder; a project could live under a hidden folder). Hide its other contents instead.
    let mut hidden: Vec<PathBuf> = canonical_existing(hidden).into_iter().filter(|h| !cwd.starts_with(h)).collect();
    if let Some(data) = data_dir.and_then(|d| std::fs::canonicalize(d).ok()) {
        if cwd.starts_with(&data) && cwd != data {
            hidden.extend(canonical_existing(
                ["nexus.db", "nexus.db-wal", "nexus.db-shm", "terminal", "scratch"].iter().map(|n| data.join(n)),
            ));
        }
    }
    Confinement { writable, hidden, network: level == Level::Restricted }
}

/// A string literal in the sandbox profile language.
fn sbpl_string(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The Seatbelt profile for a confinement.
pub fn mac_profile(c: &Confinement) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n");
    let writable: Vec<String> = c.writable.iter().map(|w| format!("(subpath {})", sbpl_string(w))).collect();
    p.push_str(&format!("(deny file-write* (require-not (require-any {} (subpath \"/dev\"))))\n", writable.join(" ")));
    for h in &c.hidden {
        p.push_str(&format!("(deny file-read* file-write* (subpath {}))\n", sbpl_string(h)));
    }
    if !c.network {
        // Outbound only to loopback; servers may listen on loopback. Note: a broad
        // `(allow network* (local ip "localhost:*"))` would also let outbound connections through
        // (the local address is not bound yet when they are checked), so only bind and inbound
        // are allowed locally.
        p.push_str(
            "(deny network*)\n(allow network* (remote ip \"localhost:*\"))\n\
             (allow network-bind (local ip \"localhost:*\"))\n(allow network-inbound (local ip \"localhost:*\"))\n",
        );
    }
    // The keychain is reached through these services, not files.
    p.push_str("(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.securityd.xpc\"))\n");
    p
}

/// The `bwrap` arguments for a confinement, up to and including `--`.
pub fn bwrap_args(c: &Confinement, cwd: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> =
        ["--die-with-parent", "--new-session", "--unshare-pid", "--unshare-ipc", "--unshare-uts", "--unshare-cgroup-try"]
            .iter()
            .map(OsString::from)
            .collect();
    if !c.network {
        a.push("--unshare-net".into());
    }
    for s in ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"] {
        a.push(s.into());
    }
    for w in &c.writable {
        a.extend(["--bind".into(), w.clone().into_os_string(), w.clone().into_os_string()]);
    }
    for h in &c.hidden {
        if h.is_dir() {
            // An empty, read-only layer: nothing can be read from the real folder, and writes fail
            // instead of landing in a throwaway layer (matching the macOS profile).
            a.extend(["--tmpfs".into(), h.clone().into_os_string(), "--remount-ro".into(), h.clone().into_os_string()]);
        } else {
            a.extend(["--ro-bind".into(), "/dev/null".into(), h.clone().into_os_string()]);
        }
    }
    a.extend(["--chdir".into(), cwd.as_os_str().to_owned(), "--".into()]);
    a
}

/// The program and arguments that run `inner` (the shell and its arguments) under `backend`.
pub fn wrap(backend: &Backend, c: &Confinement, cwd: &Path, inner: &[OsString]) -> Option<(PathBuf, Vec<OsString>)> {
    match backend {
        Backend::None => None,
        Backend::MacSandbox => {
            let mut args: Vec<OsString> = vec!["-p".into(), mac_profile(c).into()];
            args.extend(inner.iter().cloned());
            Some((PathBuf::from("/usr/bin/sandbox-exec"), args))
        }
        Backend::Bubblewrap(bwrap) => {
            let mut args = bwrap_args(c, cwd);
            args.extend(inner.iter().cloned());
            Some((bwrap.clone(), args))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-sbx-{}", crate::db::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn levels_confine_as_described() {
        let home = scratch();
        for d in [".ssh", ".aws", ".cargo/registry", ".npm", "work", "data/workspaces/run1"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::write(home.join(".npmrc"), "x").unwrap();
        std::fs::write(home.join("data/nexus.db"), "x").unwrap();
        let work = home.join("work");
        let data = home.join("data");

        assert_eq!(
            confinement(Level::Trusted, &work, Some(&data), Some(&home)),
            Confinement { writable: vec![], hidden: vec![], network: true }
        );

        let r = confinement(Level::Restricted, &work, Some(&data), Some(&home));
        assert!(r.network);
        assert!(
            r.writable.contains(&work) && r.writable.contains(&home.join(".cargo/registry")) && r.writable.contains(&home.join(".npm"))
        );
        assert!(r.hidden.contains(&home.join(".ssh")) && r.hidden.contains(&home.join(".aws")) && r.hidden.contains(&data));
        assert!(!r.hidden.contains(&home.join(".npmrc")), "package-manager settings stay readable at Restricted");

        let i = confinement(Level::Isolated, &work, Some(&data), Some(&home));
        assert!(!i.network);
        assert!(!i.writable.contains(&home.join(".npm")), "no cache writes at Isolated");
        assert!(i.hidden.contains(&home.join(".npmrc")));

        // Working in the home folder itself does not make home writable.
        let h = confinement(Level::Restricted, &home, Some(&data), Some(&home));
        assert!(!h.writable.contains(&home), "{h:?}");

        // An agent workspace inside the data folder stays visible; the database next to it does not.
        let ws = home.join("data/workspaces/run1");
        let a = confinement(Level::Restricted, &ws, Some(&data), Some(&home));
        assert!(!a.hidden.contains(&data) && a.hidden.contains(&home.join("data/nexus.db")), "{a:?}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn profiles_quote_paths_safely() {
        let c = Confinement { writable: vec![PathBuf::from("/a b/\"quoted\"\\x")], hidden: vec![PathBuf::from("/h")], network: false };
        let p = mac_profile(&c);
        assert!(p.contains(r#"(subpath "/a b/\"quoted\"\\x")"#), "{p}");
        assert!(p.contains("(deny network*)") && p.contains("localhost"));
        assert!(p.contains("(deny file-read* file-write* (subpath \"/h\"))"));
        let b = bwrap_args(&c, Path::new("/a b"));
        assert!(b.contains(&OsString::from("--unshare-net")) && b.contains(&OsString::from("--die-with-parent")));
        assert_eq!(b.last(), Some(&OsString::from("--")));
        // A hidden folder is covered by an empty layer that is then made read-only.
        let dir = std::env::temp_dir();
        let b = bwrap_args(&Confinement { writable: vec![], hidden: vec![dir.clone()], network: true }, Path::new("/"));
        let at = b.iter().position(|x| x == "--tmpfs").unwrap();
        assert_eq!(&b[at + 1..at + 4], &[dir.clone().into_os_string(), "--remount-ro".into(), dir.into_os_string()]);
    }
}
