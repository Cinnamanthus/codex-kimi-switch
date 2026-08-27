//! Codex `config.toml` takeover and restore.
//!
//! The adapter never edits Codex source code. It rewrites the *active* model
//! provider's `base_url` so Codex traffic egresses through the local adapter,
//! and it can later restore the exact pre-takeover bytes.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::Context;
use toml_edit::{DocumentMut, Item, Table, value};

const CONFIG_FILE: &str = "config.toml";
const BACKUP_FILE: &str = "config.toml.codex-kimi-switch.bak";
const MISSING_MARKER: &str = "config.toml.codex-kimi-switch.missing";
const ENV_BACKUP_FILE: &str = "config.toml.codex-kimi-switch.envbak";
const KIMI_ENV_KEY: &str = "KIMI_API_KEY";

/// Locate the Codex home directory (`CODEX_HOME`, else `~/.codex`).
pub fn default_codex_home() -> anyhow::Result<PathBuf> {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .context("cannot locate the user home directory; set CODEX_HOME")?;
    Ok(PathBuf::from(base).join(".codex"))
}

/// Backup / rewrite / restore operations against one Codex home directory.
#[derive(Debug, Clone)]
pub struct CodexConfigManager {
    home: PathBuf,
}

impl CodexConfigManager {
    /// Create a manager rooted at the given Codex home directory.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// Take over the Codex config: back up the original file once, then point
    /// the active provider's `base_url` at the local adapter. When
    /// `env_key_name` is given, the provider also gains an `env_key` entry so
    /// Codex reads its credential from that environment variable. Provider id,
    /// `wire_api`, other auth flags, and model selection stay untouched, so
    /// the desktop app's own provider/model resolution is unaffected.
    pub fn enable(&self, listen_addr: &str, env_key_name: Option<&str>) -> anyhow::Result<()> {
        let config_path = self.config_path();
        let text = match fs::read_to_string(&config_path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                anyhow::bail!(
                    "{} does not exist; nothing to take over",
                    config_path.display()
                );
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read {}", config_path.display()));
            }
        };
        let mut doc = text
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        let root = doc.as_table_mut();
        let provider_id = root
            .get("model_provider")
            .and_then(Item::as_str)
            .context("Codex config has no top-level `model_provider`")?
            .to_owned();
        let providers = root
            .get_mut("model_providers")
            .and_then(Item::as_table_mut)
            .context("Codex config has no [model_providers] table")?;
        let provider = providers
            .get_mut(provider_id.as_str())
            .and_then(Item::as_table_mut)
            .with_context(|| format!("model_providers.{provider_id} not found in Codex config"))?;
        set_string(provider, "base_url", &format!("http://{listen_addr}/v1"));
        if let Some(env_key_name) = env_key_name {
            set_string(provider, "env_key", env_key_name);
        }

        self.backup_if_needed()?;
        fs::write(&config_path, doc.to_string())
            .with_context(|| format!("failed to write {}", config_path.display()))?;
        Ok(())
    }

    /// Take over the Codex config and, when the adapter holds a Kimi key,
    /// publish it into the persistent user environment so Codex Desktop
    /// inherits a working credential on its next launch.
    pub fn enable_with_env_sync(
        &self,
        listen_addr: &str,
        api_key: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(key) = api_key {
            self.backup_env_var()?;
            crate::env_var::set_persistent(KIMI_ENV_KEY, key)?;
        }
        self.enable(listen_addr, api_key.map(|_| KIMI_ENV_KEY))
    }

    /// Restore the exact pre-takeover config and environment. Returns `true`
    /// when a takeover snapshot existed and was rolled back.
    pub fn restore(&self) -> anyhow::Result<bool> {
        let missing_marker = self.missing_marker_path();
        let backup = self.backup_path();
        let config = self.config_path();

        let mut restored = false;
        if missing_marker.exists() {
            if config.exists() {
                fs::remove_file(&config)
                    .with_context(|| format!("failed to remove {}", config.display()))?;
            }
            fs::remove_file(&missing_marker)
                .with_context(|| format!("failed to remove {}", missing_marker.display()))?;
            restored = true;
        } else if backup.exists() {
            fs::copy(&backup, &config).with_context(|| {
                format!(
                    "failed to restore {} from {}",
                    config.display(),
                    backup.display()
                )
            })?;
            fs::remove_file(&backup)
                .with_context(|| format!("failed to remove {}", backup.display()))?;
            restored = true;
        }

        let env_backup = self.env_backup_path();
        if env_backup.exists() {
            let previous = fs::read_to_string(&env_backup)
                .with_context(|| format!("failed to read {}", env_backup.display()))?;
            if previous.is_empty() {
                crate::env_var::remove_persistent(KIMI_ENV_KEY)?;
            } else {
                crate::env_var::set_persistent(KIMI_ENV_KEY, &previous)?;
            }
            fs::remove_file(&env_backup)
                .with_context(|| format!("failed to remove {}", env_backup.display()))?;
            restored = true;
        }

        Ok(restored)
    }

    fn backup_if_needed(&self) -> anyhow::Result<()> {
        if self.backup_path().exists() || self.missing_marker_path().exists() {
            return Ok(());
        }
        let config = self.config_path();
        if config.exists() {
            fs::copy(&config, self.backup_path()).with_context(|| {
                format!(
                    "failed to back up {} to {}",
                    config.display(),
                    self.backup_path().display()
                )
            })?;
        } else {
            fs::create_dir_all(&self.home)
                .with_context(|| format!("failed to create {}", self.home.display()))?;
            fs::write(self.missing_marker_path(), b"").with_context(|| {
                format!("failed to write {}", self.missing_marker_path().display())
            })?;
        }
        Ok(())
    }

    fn backup_env_var(&self) -> anyhow::Result<()> {
        let env_backup = self.env_backup_path();
        if env_backup.exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.home)
            .with_context(|| format!("failed to create {}", self.home.display()))?;
        let previous = crate::env_var::get_persistent(KIMI_ENV_KEY).unwrap_or_default();
        fs::write(&env_backup, previous)
            .with_context(|| format!("failed to write {}", env_backup.display()))?;
        Ok(())
    }

    fn env_backup_path(&self) -> PathBuf {
        self.home.join(ENV_BACKUP_FILE)
    }

    fn config_path(&self) -> PathBuf {
        self.home.join(CONFIG_FILE)
    }

    fn backup_path(&self) -> PathBuf {
        self.home.join(BACKUP_FILE)
    }

    fn missing_marker_path(&self) -> PathBuf {
        self.home.join(MISSING_MARKER)
    }
}

/// Upsert a string key. Mutating the existing entry keeps the key's decor,
/// so comments the user wrote above the key survive takeover.
fn set_string(table: &mut Table, key: &str, content: &str) {
    match table.get_mut(key) {
        Some(existing) => *existing = value(content),
        None => {
            table.insert(key, value(content));
        }
    }
}
