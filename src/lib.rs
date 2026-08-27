//! Local adapter that lets Codex talk to Kimi/Moonshot without changing Codex.

/// Codex `config.toml` takeover and restore.
pub mod codex_config;
/// Runtime settings loaded from environment variables and the config file.
pub mod config;
/// Persistent user-level environment variable synchronization.
pub mod env_var;
/// HTTP proxy surface and upstream forwarding.
pub mod proxy;
/// Moonshot Flavored JSON Schema normalization.
pub mod schema;
