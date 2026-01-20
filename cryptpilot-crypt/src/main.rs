use anyhow::{bail, Result};
use clap::Parser as _;
use cryptpilot_crypt::build;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

mod cli;
mod cmd;
mod config;

use cmd::IntoCommand;
use config::{cached::CachedVolumeConfigSource, fs::FileSystemConfigSource};

#[tokio::main]
async fn main() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    cryptpilot::fs::set_verbose(
        tracing::enabled!(target: "cryptpilot-crypt", tracing::Level::DEBUG),
    )
    .await;

    let args = cli::Cli::parse();

    if let cli::CryptSubcommand::BootService(boot_service_options) = &args.command {
        tracing::info!(
            "cryptpilot-crypt version: v{}  commit: {}  buildtime: {}",
            build::PKG_VERSION,
            build::COMMIT_HASH,
            build::BUILD_TIME
        );

        tracing::info!(
            "The cryptpilot-crypt is running in {} stage",
            boot_service_options.stage
        );
    }

    // Configure volume config source
    if let Some(config_dir) = &args.config_dir {
        let path = std::path::Path::new(config_dir);
        if !path.exists() || !path.is_dir() {
            bail!("Config dir {config_dir} does not exist or not a directory");
        }
        config::set_volume_config_source(CachedVolumeConfigSource::new(
            FileSystemConfigSource::new(config_dir.clone()),
        ))
        .await;
    }

    tracing::debug!(
        "Using config source from {:?}",
        crate::config::get_volume_config_source()
            .await
            .source_debug_string()
    );

    // Handle the command
    args.command.into_command().run().await?;

    Ok(())
}
