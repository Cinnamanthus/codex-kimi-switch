//! Full uninstall: stop running instances, restore Codex state, remove
//! adapter-owned data files.
//!
//! Uninstall never deletes the project directory or the binary itself — a
//! running executable cannot safely remove itself on Windows. The caller is
//! told exactly which directory to delete manually at the end.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::codex_config::CodexConfigManager;

/// What the uninstall actually did.
#[derive(Debug, Default)]
pub struct UninstallReport {
    /// PIDs of adapter processes that were stopped (never includes self).
    pub stopped_pids: Vec<u32>,
    /// Whether a Codex takeover snapshot was found and rolled back.
    pub codex_restored: bool,
    /// Whether the adapter config directory (holding the API key) existed and was removed.
    pub config_dir_removed: bool,
}

/// Run the full uninstall sequence.
pub fn run(
    codex_home: PathBuf,
    config_dir: &Path,
    stop_processes: bool,
) -> anyhow::Result<UninstallReport> {
    let mut report = UninstallReport::default();

    if stop_processes {
        report.stopped_pids = stop_other_instances()?;
    }

    report.codex_restored = CodexConfigManager::new(codex_home)
        .restore()
        .context("failed to restore Codex config")?;

    report.config_dir_removed = remove_config_dir(config_dir)?;

    Ok(report)
}

fn remove_config_dir(dir: &Path) -> anyhow::Result<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir).with_context(|| format!("failed to remove {}", dir.display()))?;
    Ok(true)
}

/// Stop every running `codex_kimi_switch.exe` except this process.
#[cfg(windows)]
fn stop_other_instances() -> anyhow::Result<Vec<u32>> {
    let own_pid = std::process::id();
    let output = std::process::Command::new("tasklist")
        .args([
            "/FI",
            "IMAGENAME eq codex_kimi_switch.exe",
            "/FO",
            "CSV",
            "/NH",
        ])
        .output()
        .context("failed to run tasklist")?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut stopped = Vec::new();
    for line in stdout.lines() {
        // CSV row: "codex_kimi_switch.exe","12345","Console","1","12,345 K"
        let Some(pid) = line
            .split(',')
            .nth(1)
            .map(|field| field.trim_matches('"'))
            .and_then(|field| field.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let kill = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .with_context(|| format!("failed to run taskkill for PID {pid}"))?;
        if kill.status.success() {
            stopped.push(pid);
        }
    }
    Ok(stopped)
}

/// Non-Windows stub: process management is only implemented for Windows.
#[cfg(not(windows))]
fn stop_other_instances() -> anyhow::Result<Vec<u32>> {
    Ok(Vec::new())
}
