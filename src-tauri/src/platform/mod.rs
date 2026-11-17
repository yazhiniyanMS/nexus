//! Platform layer: everything that differs between macOS, Windows and Linux lives here, so the
//! feature modules stay free of operating-system checks.
//!
//! | Service | Module |
//! | --- | --- |
//! | Operating-system description | [`info`] |
//! | Application directories | [`dirs`] |
//! | Path normalisation | [`paths`] |
//! | The user's command search path | [`env`] |
//! | Finding programs (Git, runtimes) | [`programs`] |
//! | Shell discovery | [`shell`] |
//! | Starting and stopping processes | [`process`] |
//! | Secure credential storage | [`secrets`] |
//! | Disk space | [`disk`] |
//! | Diagnostics and feedback links | [`feedback`] |
//!
//! File dialogs and notifications go through Tauri plugins, which are already cross-platform;
//! [`notify`] wraps the notification plugin so callers never touch it directly.

pub mod dirs;
pub mod disk;
pub mod env;
pub mod feedback;
pub mod notify;
pub mod paths;
pub mod process;
pub mod programs;
pub mod secrets;
pub mod shell;

use serde::Serialize;

/// What NEXUS is running on, for the About panel and bug reports. Nothing here is a secret.
#[derive(Serialize, Clone, Debug)]
pub struct PlatformInfo {
    pub app_version: &'static str,
    /// "macos", "windows" or "linux".
    pub os: &'static str,
    pub os_version: Option<String>,
    pub arch: &'static str,
    pub data_dir: String,
    /// The shell new terminals start.
    pub shell: String,
    pub git: Option<String>,
    pub git_version: Option<String>,
    /// Where API keys are kept on this system.
    pub secure_storage: &'static str,
    /// Set when NEXUS_DATA_DIR redirected the data directory.
    pub data_dir_overridden: bool,
}

pub fn info(data_dir: &std::path::Path) -> PlatformInfo {
    let git = programs::git();
    let git_version = git.as_ref().and_then(|g| {
        let mut cmd = std::process::Command::new(g);
        cmd.arg("--version").stdin(std::process::Stdio::null());
        process::hide_window(&mut cmd);
        let out = cmd.output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().trim_start_matches("git version ").to_string())
    });
    PlatformInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        os_version: crate::lab::hardware::hardware().os_version,
        arch: std::env::consts::ARCH,
        data_dir: paths::display(data_dir),
        shell: shell::interactive().0,
        git: git.map(|g| paths::display(&g)),
        git_version,
        secure_storage: secrets::BACKEND,
        data_dir_overridden: dirs::override_dir().is_some(),
    }
}

#[tauri::command]
pub fn platform_info(state: tauri::State<'_, crate::AppState>) -> PlatformInfo {
    info(&state.data_dir)
}
