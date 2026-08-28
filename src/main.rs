//! Binary entrypoint for the local Codex ↔ Kimi adapter.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use codex_kimi_switch::{
    codex_config::{CodexConfigManager, default_codex_home},
    config::Settings,
    proxy::{AppState, build_router},
};

/// Local adapter that points Codex at Kimi without touching Codex itself.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Take over the Codex config, run the proxy, and restore on exit (default).
    Run(RunArgs),
    /// Only rewrite the Codex config; do not start the proxy.
    Enable(ConfigArgs),
    /// Restore the pre-takeover Codex config and exit.
    Disable {
        /// Override the Codex home directory (default: `CODEX_HOME` or ~/.codex).
        #[arg(long)]
        codex_home: Option<PathBuf>,
    },
    /// Stop the adapter, restore Codex state, and remove all adapter data files.
    Uninstall {
        /// Override the Codex home directory (default: `CODEX_HOME` or ~/.codex).
        #[arg(long)]
        codex_home: Option<PathBuf>,
    },
}

/// Shared configuration flags.
#[derive(Debug, Default, Clone, clap::Args)]
struct ConfigArgs {
    /// Local listen address of the adapter.
    #[arg(long)]
    listen_addr: Option<String>,
    /// Moonshot/Kimi upstream base URL.
    #[arg(long)]
    upstream_base: Option<String>,
    /// Kimi API key held by the adapter; replaces the client Authorization header.
    #[arg(long)]
    api_key: Option<String>,
    /// Override the Codex home directory (default: `CODEX_HOME` or ~/.codex).
    #[arg(long)]
    codex_home: Option<PathBuf>,
}

/// Flags for the default run command.
#[derive(Debug, Default, clap::Args)]
struct RunArgs {
    #[command(flatten)]
    config: ConfigArgs,
    /// Keep the rewritten Codex config when the proxy exits.
    #[arg(long = "no-restore-on-exit")]
    no_restore_on_exit: bool,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli
        .command
        .unwrap_or_else(|| Command::Run(RunArgs::default()))
    {
        Command::Run(args) => run(args).await,
        Command::Enable(args) => {
            let settings = settings_from(&args);
            warn_without_key(&settings);
            let manager = CodexConfigManager::new(codex_home(args.codex_home)?);
            manager.enable_with_env_sync(&settings.listen_addr, settings.api_key.as_deref())?;
            tracing::info!(
                addr = %settings.listen_addr,
                "codex config takeover enabled"
            );
            Ok(())
        }
        Command::Disable { codex_home } => {
            let manager = CodexConfigManager::new(self::codex_home(codex_home)?);
            if manager.restore()? {
                tracing::info!("codex config restored to pre-takeover state");
            } else {
                tracing::info!("no takeover state found; nothing to restore");
            }
            Ok(())
        }
        Command::Uninstall { codex_home } => {
            let codex_home = self::codex_home(codex_home)?;
            let config_dir = codex_kimi_switch::config::config_dir()
                .context("cannot locate the user home directory")?;
            let report = codex_kimi_switch::uninstall::run(codex_home, &config_dir, true)?;
            tracing::info!(
                stopped_pids = ?report.stopped_pids,
                codex_restored = report.codex_restored,
                config_dir_removed = report.config_dir_removed,
                "uninstall complete"
            );
            tracing::info!(
                "adapter data is gone; to finish, delete the project directory manually \
                 (the folder containing this exe, typically the repository root)"
            );
            Ok(())
        }
    }
}

async fn run(args: RunArgs) -> anyhow::Result<()> {
    let settings = settings_from(&args.config);
    warn_without_key(&settings);
    let manager = CodexConfigManager::new(codex_home(args.config.codex_home.clone())?);
    manager.enable_with_env_sync(&settings.listen_addr, settings.api_key.as_deref())?;
    tracing::info!(
        addr = %settings.listen_addr,
        "codex config takeover active"
    );

    let serve_result = serve(settings).await;
    let restore_result = if args.no_restore_on_exit {
        Ok(false)
    } else {
        manager.restore()
    };
    serve_result?;
    restore_result?;
    Ok(())
}

async fn serve(settings: Settings) -> anyhow::Result<()> {
    let listen_addr = settings.listen_addr.clone();
    let state = AppState::new(settings)?;
    let app = build_router(state.clone());
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;

    tracing::info!(
        addr = %listen_addr,
        upstream = %state.settings().upstream_base,
        "codex_kimi_switch listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn settings_from(args: &ConfigArgs) -> Settings {
    let mut settings = Settings::load();
    if let Some(value) = &args.listen_addr {
        settings.listen_addr.clone_from(value);
    }
    if let Some(value) = &args.upstream_base {
        settings.upstream_base.clone_from(value);
    }
    if let Some(value) = &args.api_key {
        settings.api_key = Some(value.clone());
    }
    settings
}

fn warn_without_key(settings: &Settings) {
    if settings.api_key.is_none() {
        tracing::warn!(
            "no Kimi API key configured; set `api_key` in codex_kimi_switch.toml or use \
             --api-key / KIMI_API_KEY — client credentials will be forwarded unchanged"
        );
    }
}

fn codex_home(overridden: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    overridden.map_or_else(default_codex_home, Ok)
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
