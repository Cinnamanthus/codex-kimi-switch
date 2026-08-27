//! File- and environment-backed settings for the adapter.
//!
//! Precedence (highest first): CLI flag → config file → environment variable
//! → built-in default. The config file outranks the environment because
//! ambient env vars are exactly the kind of stale, invisible state that
//! caused silent key mismatches. The config file lives in exactly one place —
//! `%USERPROFILE%\.codex-kimi-switch\config.toml` — so there is no way for
//! two copies to drift apart.

use std::path::PathBuf;

const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_UPSTREAM_BASE: &str = "https://api.kimi.com/coding/v1";
const CONFIG_DIR_NAME: &str = ".codex-kimi-switch";
const CONFIG_FILE_NAME: &str = "config.toml";

/// Runtime configuration for the local adapter.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Local address the adapter listens on.
    pub listen_addr: String,
    /// Moonshot/Kimi base URL that receives the forwarded traffic.
    pub upstream_base: String,
    /// Kimi API key held by the adapter; replaces the client Authorization header.
    pub api_key: Option<String>,
    /// Force schema normalization even when the upstream URL does not look like Moonshot.
    pub sanitize_always: bool,
}

impl Settings {
    /// Load settings: environment variables layered over the config file.
    pub fn load() -> Self {
        let file = FileConfig::load();
        Self {
            listen_addr: env_nonempty("CODEX_KIMI_LISTEN_ADDR")
                .or(file.listen_addr)
                .unwrap_or_else(|| DEFAULT_LISTEN_ADDR.to_owned()),
            upstream_base: env_nonempty("CODEX_KIMI_UPSTREAM_BASE")
                .or(file.upstream_base)
                .unwrap_or_else(|| DEFAULT_UPSTREAM_BASE.to_owned()),
            api_key: file.api_key.or_else(|| env_nonempty("KIMI_API_KEY")),
            sanitize_always: env_flag("CODEX_KIMI_SANITIZE_ALWAYS"),
        }
    }

    /// Whether this upstream should receive Moonshot schema normalization.
    pub fn should_sanitize(&self) -> bool {
        if self.sanitize_always {
            return true;
        }
        let upstream = self.upstream_base.to_ascii_lowercase();
        upstream.contains("kimi") || upstream.contains("moonshot")
    }
}

/// Settings read from the on-disk config file.
#[derive(Debug, Default)]
struct FileConfig {
    listen_addr: Option<String>,
    upstream_base: Option<String>,
    api_key: Option<String>,
}

impl FileConfig {
    fn load() -> Self {
        let Some(path) = config_file_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => {
                let table = doc.as_table();
                Self {
                    listen_addr: read_string(table, "listen_addr"),
                    upstream_base: read_string(table, "upstream_base"),
                    api_key: read_string(table, "api_key"),
                }
            }
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "config file exists but failed to parse; ignoring it"
                );
                Self::default()
            }
        }
    }
}

fn config_file_path() -> Option<PathBuf> {
    let home = env_nonempty("USERPROFILE").or_else(|| env_nonempty("HOME"))?;
    Some(
        PathBuf::from(home)
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME),
    )
}

fn read_string(table: &toml_edit::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(|item| item.as_str())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
