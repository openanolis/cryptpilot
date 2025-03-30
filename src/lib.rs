pub mod cli;
pub mod cmd;
pub mod config;
pub mod fs;
pub mod measure;
pub mod provider;
pub mod types;

use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Parser as _;
use cli::{BootServiceOptions, BootStage};
use cmd::boot_service::{
    copy_config::copy_config_to_initrd_state, initrd_state::InitrdStateConfigSource,
};
use config::source::{cached::CachedConfigSource, fs::FileSystemConfigSource};
use log::{debug, info};
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

shadow!(build);

pub async fn run() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    // let filter = tracing_subscriber::filter::LevelFilter::INFO;
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = cli::Args::parse();

    if let cli::Command::BootService(boot_service_options) = &args.command {
        info!(
            "cryptpilot version: v{}  commit: {}  buildtime: {}",
            build::PKG_VERSION,
            build::COMMIT_HASH,
            build::BUILD_TIME
        );

        info!(
            "The cryptpilot is running in {} stage",
            boot_service_options.stage
        );
    }

    // Handle config dir
    match args.command {
        cli::Command::BootService(BootServiceOptions {
            stage: BootStage::InitrdBeforeSysroot,
        }) => {
            // We should load the configs from unsafe space and save them to initrd state for using later.
            copy_config_to_initrd_state().await?;
            config::source::set_config_source(CachedConfigSource::new(
                InitrdStateConfigSource::new(),
            ))
            .await;
        }
        cli::Command::BootService(BootServiceOptions {
            stage: BootStage::InitrdAfterSysroot,
        }) => {
            config::source::set_config_source(CachedConfigSource::new(
                InitrdStateConfigSource::new(),
            ))
            .await;
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
            }
            // Or use default config dir.
        }
    }

    // Check verbose option from config file, if is running as boot service.
    if let cli::Command::BootService(_) = args.command {
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

            info!("Log level set to DEBUG");
        }
    }

    debug!(
        "Using config source from {:?}",
        crate::config::source::get_config_source()
            .await
            .source_debug_string()
    );

    match args.command {
        cli::Command::Show(_) => cmd::show::cmd_show().await?,
        cli::Command::Init(init_options) => cmd::init::cmd_init(&init_options).await?,
        cli::Command::Open(open_options) => cmd::open::cmd_open(&open_options).await?,
        cli::Command::Close(close_options) => cmd::close::cmd_close(&close_options).await?,
        cli::Command::DumpConfig => cmd::dump_config::cmd_dump_config().await?,
        cli::Command::BootService(boot_service_options) => {
            cmd::boot_service::cmd_boot_service(&boot_service_options).await?
        }
    };

    Ok(())
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
