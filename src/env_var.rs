//! Persistent user-level environment variable synchronization (Windows).
//!
//! Codex Desktop inherits its environment from the shell, so the adapter
//! writes the Kimi key into the user's persistent environment
//! (`HKCU\Environment`) through the .NET API, which also broadcasts the
//! change so newly launched apps pick it up.

/// Read a persistent user-level environment variable (`None` if unset).
#[cfg(windows)]
pub fn get_persistent(name: &str) -> Option<String> {
    let script = format!(
        "[Environment]::GetEnvironmentVariable('{}','User')",
        escape(name)
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
}

/// Set (or overwrite) a persistent user-level environment variable.
#[cfg(windows)]
pub fn set_persistent(name: &str, value: &str) -> anyhow::Result<()> {
    run_ps(&format!(
        "[Environment]::SetEnvironmentVariable('{}','{}','User')",
        escape(name),
        escape(value)
    ))
}

/// Remove a persistent user-level environment variable.
#[cfg(windows)]
pub fn remove_persistent(name: &str) -> anyhow::Result<()> {
    run_ps(&format!(
        "[Environment]::SetEnvironmentVariable('{}',$null,'User')",
        escape(name)
    ))
}

#[cfg(windows)]
fn run_ps(script: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .context("failed to launch powershell")?;
    if !output.status.success() {
        anyhow::bail!(
            "environment update failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

/// Non-Windows stub: there is no persistent registry environment to read.
#[cfg(not(windows))]
pub fn get_persistent(_name: &str) -> Option<String> {
    None
}

/// Non-Windows stub: no-op.
#[cfg(not(windows))]
pub fn set_persistent(_name: &str, _value: &str) -> anyhow::Result<()> {
    Ok(())
}

/// Non-Windows stub: no-op.
#[cfg(not(windows))]
pub fn remove_persistent(_name: &str) -> anyhow::Result<()> {
    Ok(())
}
