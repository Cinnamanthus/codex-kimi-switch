//! Tests for the uninstall workflow.

use std::fs;

use codex_kimi_switch::{codex_config::CodexConfigManager, uninstall};

const ORIGINAL: &str = concat!(
    "model_provider = \"custom\"\n",
    "\n",
    "[model_providers.custom]\n",
    "name = \"kimi\"\n",
    "base_url = \"https://api.kimi.com/coding/v1\"\n",
    "wire_api = \"responses\"\n",
);

#[test]
fn uninstall_restores_codex_and_removes_config_dir() -> Result<(), Box<dyn std::error::Error>> {
    // Given: a taken-over Codex home and an adapter config dir holding a key.
    let codex_dir = tempfile::tempdir()?;
    fs::write(codex_dir.path().join("config.toml"), ORIGINAL)?;
    CodexConfigManager::new(codex_dir.path()).enable("127.0.0.1:8787", None)?;

    let config_dir = tempfile::tempdir()?;
    fs::write(
        config_dir.path().join("config.toml"),
        "api_key = \"sk-kimi-...\"\n",
    )?;

    // When: uninstall runs (process stopping is exercised manually, not here).
    let report = uninstall::run(codex_dir.path().to_path_buf(), config_dir.path(), false)?;

    // Then: Codex config is byte-identical to the pre-takeover state and the
    // adapter's own data directory (with the key) is gone.
    assert!(report.codex_restored);
    assert!(report.config_dir_removed);
    assert!(report.stopped_pids.is_empty());
    assert_eq!(
        fs::read_to_string(codex_dir.path().join("config.toml"))?,
        ORIGINAL
    );
    assert!(!config_dir.path().join("config.toml").exists());
    Ok(())
}

#[test]
fn uninstall_without_anything_is_a_clean_noop() -> Result<(), Box<dyn std::error::Error>> {
    // Given: nothing was ever taken over and the config dir does not exist.
    let codex_dir = tempfile::tempdir()?;
    let config_dir = codex_dir.path().join("nonexistent-config-dir");

    // When/Then: uninstall reports nothing to do and changes nothing.
    let report = uninstall::run(codex_dir.path().to_path_buf(), &config_dir, false)?;
    assert!(!report.codex_restored);
    assert!(!report.config_dir_removed);
    assert!(report.stopped_pids.is_empty());
    Ok(())
}
