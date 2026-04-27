use anyhow::{Context as _, Result};
use clap::Parser as _;
use cryptpilot_fde::cli::{GuestCli, GuestSubcommand};
use cryptpilot_fde::cmd::boot_service::copy_config::copy_config_to_initrd_state_if_not_exist;
use cryptpilot_fde::cmd::{Command, GuestBootServiceCommand};
use cryptpilot_fde::config::{
    cached::CachedFdeConfigSource, initrd_state::InitrdStateConfigSource,
};
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

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

    cryptpilot::fs::set_verbose(
        tracing::enabled!(target: "cryptpilot-fde-guest", tracing::Level::DEBUG),
    )
    .await;

    let args = GuestCli::parse();

    let GuestSubcommand::BootService(boot_service_options) = &args.command;

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

    // Load config from unsafe space and save to initrd state for later use.
    copy_config_to_initrd_state_if_not_exist(true).await?;
    cryptpilot_fde::config::set_fde_config_source(CachedFdeConfigSource::new(
        InitrdStateConfigSource::new(),
    ))
    .await;

    // Check verbose option from config file.
    let global_config = cryptpilot_fde::config::get_fde_config_source()
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

    tracing::debug!(
        "Using config source from {:?}",
        cryptpilot_fde::config::get_fde_config_source()
            .await
            .source_debug_string()
    );

    let cmd = GuestBootServiceCommand {
        boot_service_options: boot_service_options.clone(),
    };
    cmd.run().await?;

    Ok(())
}
