//! What this machine is, so benchmark results can say where they were measured.
//! Every field is read from the operating system; unknown values stay unknown.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Hardware {
    pub os: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub cpu: Option<String>,
    pub cores: Option<usize>,
    pub memory_bytes: Option<u64>,
}

/// Output of a fixed system command, or None. Never takes input from outside this file.
#[cfg(target_os = "macos")]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// First value of `key` in "key : value" / "key=value" text such as /proc/cpuinfo.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn field(text: &str, key: &str, separator: char) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once(separator)?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty())
    })
}

fn detect() -> Hardware {
    let mut hw = Hardware {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cores: std::thread::available_parallelism().ok().map(|n| n.get()),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    {
        hw.cpu = command_output("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]);
        hw.memory_bytes = command_output("/usr/sbin/sysctl", &["-n", "hw.memsize"]).and_then(|v| v.parse().ok());
        hw.os_version = command_output("/usr/bin/sw_vers", &["-productVersion"]);
    }
    #[cfg(target_os = "linux")]
    {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        hw.cpu = field(&cpuinfo, "model name", ':').or_else(|| field(&cpuinfo, "Model", ':'));
        let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        hw.memory_bytes = field(&meminfo, "MemTotal", ':')
            .and_then(|v| v.split_whitespace().next().and_then(|n| n.parse::<u64>().ok()))
            .map(|kb| kb * 1024);
        hw.os_version = std::fs::read_to_string("/etc/os-release").ok().and_then(|t| field(&t, "PRETTY_NAME", '='));
    }
    #[cfg(target_os = "windows")]
    {
        hw.cpu = std::env::var("PROCESSOR_IDENTIFIER").ok().filter(|v| !v.is_empty());
    }
    hw
}

/// This machine's description, read once.
pub fn hardware() -> Hardware {
    static CACHE: OnceLock<Hardware> = OnceLock::new();
    CACHE.get_or_init(detect).clone()
}

#[tauri::command]
pub fn lab_hardware() -> Hardware {
    hardware()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fields_from_proc_style_text() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen 7 5800X 8-Core Processor\nmodel name\t: other\n";
        assert_eq!(field(cpuinfo, "model name", ':').as_deref(), Some("AMD Ryzen 7 5800X 8-Core Processor"));
        assert_eq!(
            field("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n", "PRETTY_NAME", '=').as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert_eq!(field("MemTotal:       32768000 kB\n", "MemTotal", ':').as_deref(), Some("32768000 kB"));
        assert_eq!(field(cpuinfo, "missing", ':'), None);
    }

    #[test]
    fn describes_this_machine_without_inventing_values() {
        let hw = hardware();
        assert_eq!(hw.os, std::env::consts::OS);
        assert!(!hw.arch.is_empty());
        assert!(hw.cores.is_some_and(|c| c >= 1));
        // Memory, when reported, is a plausible amount of RAM rather than a placeholder.
        if let Some(bytes) = hw.memory_bytes {
            assert!(bytes > 256 * 1024 * 1024, "{bytes}");
        }
        assert_eq!(hardware(), hw, "cached");
    }
}
