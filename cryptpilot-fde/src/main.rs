use std::path::Path;

use anyhow::{bail, Context as _, Result};
use clap::Parser as _;
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

mod cli;
mod cmd;
mod config;
mod disk;

use cli::BootServiceOptions;
use cmd::boot_service::copy_config::copy_config_to_initrd_state_if_not_exist;
use config::{cached::CachedFdeConfigSource, initrd_state::InitrdStateConfigSource};

use crate::cmd::IntoCommand;

shadow!(build);

#[tokio::main]
async fn main() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    cryptpilot::fs::set_verbose(tracing::enabled!(target: "cryptpilot-fde", tracing::Level::DEBUG))
        .await;

    let args = cli::Cli::parse();

    if let cli::FdeSubcommand::BootService(boot_service_options) = &args.command {
        tracing::info!(
            "cryptpilot-fde version: v{}  commit: {}  buildtime: {}",
            build::PKG_VERSION,
            build::COMMIT_HASH,
            build::BUILD_TIME
        );

        tracing::info!(
            "The cryptpilot-fde is running in {} stage",
            boot_service_options.stage
        );
    }

    match &args.command {
        cli::FdeSubcommand::BootService(BootServiceOptions { stage: _ }) => {
            // We should load the configs from unsafe space and save them to initrd state for using later.
            copy_config_to_initrd_state_if_not_exist(true).await?;
            config::set_fde_config_source(CachedFdeConfigSource::new(
                InitrdStateConfigSource::new(),
            ))
            .await;
        }
        cli::FdeSubcommand::ShowReferenceValue(_) | cli::FdeSubcommand::Config(_) => {
            if args.config_dir.is_some() {
                bail!("Cannot specify `--config-dir` with `show-reference-value` or `config` subcommand");
            }

            if Path::new("/etc/initrd-release").exists() {
                // If we are in initrd, copy config to initrd state and load it from there, so we can run cryptpilot commands manually in initrd in case we need to operate in emergency shell.
                copy_config_to_initrd_state_if_not_exist(false).await?;
                config::set_fde_config_source(CachedFdeConfigSource::new(
                    InitrdStateConfigSource::new(),
                ))
                .await;
            }
        }
    }

    // Check verbose option from config file, if is running as boot service.
    if let cli::FdeSubcommand::BootService(_) = args.command {
        let global_config = crate::config::get_fde_config_source()
            .await
            .get_global_config()
            .await?;
        if global_config
            .and_then(|global_config| global_config.boot)
            .map(|boot| boot.verbose)
            .unwrap_or(false)
        {
            reload_handle
                .modify(|filter| {
                    *filter = tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "debug".into())
                })
                .context("Failed to update log level to DEBUG")?;
            cryptpilot::fs::set_verbose(true).await;

            tracing::info!("Log level set to DEBUG");
        }
    }

    tracing::debug!(
        "Using config source from {:?}",
        crate::config::get_fde_config_source()
            .await
            .source_debug_string()
    );

    // Handle the command
    args.command.into_command().run().await?;

    Ok(())
}
