#![deny(clippy::disallowed_methods)]

pub mod cli;
pub mod cmd;
pub mod config;
pub mod fs;
pub mod measure;
pub mod provider;
pub mod types;
pub mod vendor;

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Parser as _;
use cli::{BootServiceOptions, BootStage, FdeOptions, GlobalSubcommand};
use cmd::{boot_service::copy_config::copy_config_to_initrd_state_if_not_exist, IntoCommand as _};
use config::source::{
    cached::CachedConfigSource, fs::FileSystemConfigSource, initrd_state::InitrdStateConfigSource,
};
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

shadow!(build);

pub async fn run() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    crate::fs::set_verbose(tracing::enabled!(target: "cryptpilot", tracing::Level::DEBUG)).await;

    let args = cli::Cli::parse();

    if let cli::GlobalSubcommand::BootService(boot_service_options) = &args.command {
        tracing::info!(
            "cryptpilot version: v{}  commit: {}  buildtime: {}",
            build::PKG_VERSION,
            build::COMMIT_HASH,
            build::BUILD_TIME
        );

        tracing::info!(
            "The cryptpilot is running in {} stage",
            boot_service_options.stage
        );
    }

    // Handle config dir
    match &args.command {
        cli::GlobalSubcommand::BootService(BootServiceOptions { stage }) => {
            if matches!(
                stage,
                BootStage::InitrdFdeBeforeSysroot | BootStage::InitrdFdeAfterSysroot
            ) {
                // We should load the configs from unsafe space and save them to initrd state for using later.
                copy_config_to_initrd_state_if_not_exist(true).await?;
                config::source::set_config_source(CachedConfigSource::new(
                    InitrdStateConfigSource::new(),
                ))
                .await;
            }
        }
        GlobalSubcommand::Fde(FdeOptions { .. }) => {
            if args.config_dir.is_some() {
                bail!("Cannot specify `--config-dir` with `fde` subcommand")
            }
        }
        _ => {
            // Set to the given config dir from cryptpilot command line.
            if let Some(config_dir) = args.config_dir {
                if !Path::new(&config_dir).exists() || !Path::new(&config_dir).is_dir() {
                    bail!("Config dir {config_dir} does not exist or not a directory")
                }

                config::source::set_config_source(CachedConfigSource::new(
                    FileSystemConfigSource::new(config_dir),
                ))
                .await;
            } else if Path::new("/etc/initrd-release").exists() {
                // If we are in initrd, copy config to initrd state and load it from there, so we can run cryptpilot commands manually in initrd in case we need to operate in emergency shell.
                copy_config_to_initrd_state_if_not_exist(false).await?;
                config::source::set_config_source(CachedConfigSource::new(
                    InitrdStateConfigSource::new(),
                ))
                .await;
            } else {
                // Or use default config dir.
            }
        }
    }

    // Check verbose option from config file, if is running as boot service.
    if let cli::GlobalSubcommand::BootService(_) = args.command {
        let global_config = crate::config::source::get_config_source()
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
            crate::fs::set_verbose(true).await;

            tracing::info!("Log level set to DEBUG");
        }
    }

    tracing::debug!(
        "Using config source from {:?}",
        crate::config::source::get_config_source()
            .await
            .source_debug_string()
    );

    // Handle the command
    args.command.into_command().run().await?;

    Ok(())
}

/// A macro like scopeguard::defer! but can defer a future.
///
/// Note that other code running concurrently in the same task will be suspended
/// due to the call to block_in_place, until the future is finished.
///
/// # Examples
///
/// ```ignore
/// async_defer!(async {
///     // Do some cleanup
/// });
/// ```
///
/// # Panics
///
/// This macro should only be used in tokio multi-thread runtime, and will panics
/// if called from a [`current_thread`] runtime.
///
#[macro_export]
macro_rules! async_defer {
    ($future:expr) => {
        scopeguard::defer! {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let _ = $future.await;
                });
            });
        }
    };
}

#[cfg(test)]
mod tests {

    use super::*;

    #[cfg(test)]
    #[ctor::ctor]
    fn init() {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "debug".into());
        let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}
