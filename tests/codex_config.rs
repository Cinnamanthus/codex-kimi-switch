//! Tests for Codex `config.toml` takeover and restore.

use std::fs;
use std::path::Path;

use codex_kimi_switch::codex_config::CodexConfigManager;

const ORIGINAL: &str = concat!(
    "# hand-written codex config\n",
    "model_provider = \"custom\"\n",
    "model = \"kimi-k3\"\n",
    "\n",
    "[model_providers.custom]\n",
    "name = \"kimi\"\n",
    "base_url = \"https://api.kimi.com/coding/v1\"\n",
    "wire_api = \"responses\"\n",
    "requires_openai_auth = true\n",
);

fn read_config(home: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(home.join("config.toml"))?)
}

#[test]
fn enable_rewrites_only_active_provider_base_url() -> Result<(), Box<dyn std::error::Error>> {
    // Given: a Codex config whose active provider talks to Kimi directly.
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("config.toml"), ORIGINAL)?;
    let manager = CodexConfigManager::new(dir.path());

    // When: the adapter takes over.
    manager.enable("127.0.0.1:8787", Some("KIMI_API_KEY"))?;

    // Then: only the active provider's base_url changed; provider id, wire_api,
    // auth flags, model selection, and comments are all preserved.
    let managed_config = read_config(dir.path())?;
    assert!(managed_config.contains("base_url = \"http://127.0.0.1:8787/v1\""));
    assert!(managed_config.contains("env_key = \"KIMI_API_KEY\""));
    assert!(managed_config.contains("model_provider = \"custom\""));
    assert!(managed_config.contains("model = \"kimi-k3\""));
    assert!(managed_config.contains("wire_api = \"responses\""));
    assert!(managed_config.contains("requires_openai_auth = true"));
    assert!(managed_config.contains("# hand-written codex config"));
    assert!(!managed_config.contains("kimi_local"));

    // When: a second takeover happens before restore...
    manager.enable("127.0.0.1:9999", None)?;
    assert!(read_config(dir.path())?.contains("base_url = \"http://127.0.0.1:9999/v1\""));

    // Then: restore still returns the exact pre-takeover bytes.
    assert!(manager.restore()?);
    assert_eq!(read_config(dir.path())?, ORIGINAL);
    Ok(())
}

#[test]
fn enable_without_active_provider_fails_without_side_effects()
-> Result<(), Box<dyn std::error::Error>> {
    // Given: a config with no top-level model_provider.
    let dir = tempfile::tempdir()?;
    let original = "[model_providers.custom]\nname = \"kimi\"\n";
    fs::write(dir.path().join("config.toml"), original)?;
    let manager = CodexConfigManager::new(dir.path());

    // When/Then: takeover fails and leaves neither config edits nor backups.
    assert!(manager.enable("127.0.0.1:8787", None).is_err());
    assert_eq!(read_config(dir.path())?, original);
    assert!(
        !dir.path()
            .join("config.toml.codex-kimi-switch.bak")
            .exists()
    );
    Ok(())
}

#[test]
fn enable_without_config_file_fails() -> Result<(), Box<dyn std::error::Error>> {
    // Given: a Codex home with no config.toml at all.
    let dir = tempfile::tempdir()?;
    let manager = CodexConfigManager::new(dir.path());

    // When/Then: takeover fails instead of inventing a config.
    assert!(manager.enable("127.0.0.1:8787", None).is_err());
    assert!(!dir.path().join("config.toml").exists());
    Ok(())
}

#[test]
fn restore_without_takeover_is_a_noop() -> Result<(), Box<dyn std::error::Error>> {
    // Given: a Codex home that was never taken over.
    let dir = tempfile::tempdir()?;
    let manager = CodexConfigManager::new(dir.path());

    // When/Then: restore reports nothing to do and changes nothing.
    assert!(!manager.restore()?);
    assert!(!dir.path().join("config.toml").exists());
    Ok(())
}
